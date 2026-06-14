//! `legaia-rando` — top-level randomizer / disc patcher CLI.
//!
//! Reads a **user-supplied** retail Legaia disc image, plans a randomization
//! from a seed, and emits a portable PPF 3.0 patch (the redistributable
//! deliverable) plus, optionally, a full patched `.bin` copy for local play.
//! The patched `.bin` contains Sony bytes and must never be shared — the
//! shareable artifacts are the patcher and the seed.
//!
//! ```text
//! legaia-rando randomize --input DISC.bin --seed mysrun --drops shuffle --patch out.ppf
//! legaia-rando drops     --input DISC.bin      # read-only: list current monster drops
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::ppf;

#[derive(Parser)]
#[command(
    name = "legaia-rando",
    about = "Legend of Legaia randomizer / disc patcher (operates on a user-supplied disc)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    /// tiles the house-door shuffle would touch, grouped by scene — the
    /// cross-context player MOVE_TOs in named partition-0 door records (NPC /
    /// cutscene movement is excluded by construction).
    HouseDoors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show the new game's current starting inventory (the
    /// `(item, count)` slots a New Game begins with — vanilla is Healing Leaf
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
    /// DEF↑ / DEF↓ / AGL / SPD) from the `battle_data` archive — the population
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
    /// table — the population the `--spell-cost` randomizer redistributes.
    SpellCosts {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the equipment stat-bonus table (`DAT_80074F68`), grouped
    /// by slot category, with each row's stats and the items that reference it —
    /// the population the `--equip-bonus` randomizer redistributes.
    EquipBonuses {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show each character's current favored weapon class (read from
    /// the player battle files) — what the `--weapon-specialty` randomizer
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
}

#[derive(Parser)]
struct RandomizeArgs {
    /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
    #[arg(long)]
    input: PathBuf,
    /// Seed for reproducibility. Either a number (decimal or `0x`-hex) or any
    /// string (hashed to a number). The resolved numeric seed is always
    /// printed so a run can be reproduced exactly. If omitted, one is drawn
    /// from the system clock.
    #[arg(long)]
    seed: Option<String>,
    /// How monster item drops are reassigned. Ignored when `--equipment-drops`
    /// is set (that pass owns the drop slot).
    #[arg(long, value_enum, default_value_t = DropArg::Shuffle)]
    drops: DropArg,
    /// Turn every monster's drop into a *rare* random piece of equipment
    /// (weapon / armor / accessory) instead of the normal drop. The chance is
    /// tiered by the gear's value and the enemy's strength (the rarer of the
    /// two wins): early-game ~3 %, late-game ~1 % (the engine's integer
    /// `rand() % 100` roll can't express the requested sub-percent 0.5 %).
    /// Takes precedence over `--drops`.
    #[arg(long, default_value_t = false)]
    equipment_drops: bool,
    /// How random-encounter formations are reassigned. The pool each scene draws
    /// from is set by `--encounter-scope`.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    encounters: DropArg,
    /// Pool an encounter randomization draws from: `scene` (each scene's own
    /// monsters — the default, every monster stays a local resident),
    /// `kingdom` (any monster from the same kingdom: Drake / Sebucus / Karisto),
    /// or `world` (any monster on the disc, so late-game monsters can appear at
    /// the start). Only applies when `--encounters` is set. `kingdom` needs the
    /// disc's CDNAME.TXT; the wider pools rely on the battle loader streaming a
    /// monster by id, so an out-of-area enemy still loads and renders.
    #[arg(long, value_enum, default_value_t = ScopeArg::Scene)]
    encounter_scope: ScopeArg,
    /// How treasure-chest contents are reassigned (global; `random` draws from
    /// the valid item pool, `shuffle` redistributes the existing chest items).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    chests: DropArg,
    /// How town-merchant shops are reassigned — what stores sell (global;
    /// `shuffle` redistributes the existing shop-item multiset across all towns,
    /// `random` draws each slot from the valid item pool). The town shop stock is
    /// inline in each scene's field-VM script (op `0x49`).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    shops: DropArg,
    /// How the casino prize-exchange is reassigned (`shuffle` redistributes the
    /// existing prizes, `random` draws from the existing prize pool; each prize
    /// keeps its coin price + progression gate). Distinct from `--shops`: the
    /// casino spends coins, not gold.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    casino: DropArg,
    /// How monster combat stats are reassigned (HP / MP / ATK / DEF / AGL / SPD
    /// from the `battle_data` archive). `shuffle` permutes each stat column
    /// across the roster (each stat's multiset preserved, so the overall
    /// difficulty budget is kept); `random` draws each stat from that column's
    /// pool. Spirit/SP is left untouched. `legaia-rando monster-stats` lists the
    /// current stats.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    monster_stats: DropArg,
    /// How special-attack power is reassigned (the battle-action move-power
    /// table — enemy specials + Seru-magic, NOT party Tactical Arts). `shuffle`
    /// permutes the 44 power values (multiset preserved); `random` draws each
    /// from that pool. Only the power changes — each move keeps its own
    /// animation, effects, and sound. `legaia-rando move-powers` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    move_power: DropArg,
    /// How the element-affinity matrix is reassigned (which element beats which:
    /// the 8×8 damage-scale grid). `shuffle` permutes the 64 cells (the same
    /// number of weaknesses / resistances exists, between different pairs);
    /// `random` draws each cell from that pool. Per-character element assignment
    /// is left untouched. `legaia-rando affinity` shows the current grid.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    element_affinity: DropArg,
    /// How spell MP costs are reassigned (the SCUS spell table). `shuffle`
    /// permutes the MP costs of the named, costed spells (the cost multiset is
    /// preserved); `random` draws each from that pool. Free / internal-tier
    /// spells never gain a cost. `legaia-rando spell-costs` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    spell_cost: DropArg,
    /// How equipment passive stat bonuses are reassigned (the SCUS bonus table).
    /// `shuffle` permutes each slot category's stat tuples (`INT/ATK/UDF/LDF/SPD`)
    /// among that category's gear (so weapon power lands on another weapon, armor
    /// on armor; the per-category budget is kept); `random` draws each from that
    /// category's pool. The equip-character mask, accessory passive, and slot type
    /// never move. `legaia-rando equip-bonuses` lists the current table.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    equip_bonus: DropArg,
    /// Reassign which weapon class each character specializes in (Vahn blades,
    /// Noa claws, Gala clubs/axes by default). Permutes the three favored
    /// families among the characters and rewrites the per-(character, weapon)
    /// arm-cost byte in the player battle files, so an off-class weapon widens
    /// the Arms command in an arts combo. The Astral Sword stays always-wide.
    /// `legaia-rando weapon-specialty` shows the current favored class per char.
    #[arg(long, default_value_t = false)]
    weapon_specialty: bool,
    /// How per-monster steal items are reassigned (the Evil God Icon table;
    /// `shuffle` redistributes the existing steal items, `random` draws from the
    /// valid item pool — the steal *chance* is always preserved).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    steals: DropArg,
    /// How each Tactical Art's button combo is reassigned. `shuffle` permutes a
    /// character's own combos among its arts; `random` draws each art a combo
    /// from the global pool of every regular art's combo. Either way every art
    /// keeps a combo that's unique within its character, and the Miracle Art is
    /// left untouched. `legaia-rando arts` lists current combos.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    arts: DropArg,
    /// How scene-transition doors/exits are reassigned (one-way / decoupled:
    /// each door's whole destination — scene + entry tile + facing — is
    /// reassigned globally; `shuffle` permutes the existing destinations across
    /// all doors, `random` draws each from the global pool). Going back through
    /// the destination's own doors is not guaranteed to return you.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    doors: DropArg,
    /// Whether door randomization is bidirectional (`coupled`: re-pair doors so
    /// you can return the way you came) or one-way (`decoupled`: each door's
    /// destination is independent, so going back leads elsewhere). Only applies
    /// when `--doors` is not `none`.
    #[arg(long, value_enum, default_value_t = CouplingArg::Coupled)]
    door_coupling: CouplingArg,
    /// How intra-town (house / interior) doors are reassigned. Only `shuffle`
    /// is meaningful (a per-scene, class-preserving shuffle of the player
    /// door-warp target tiles: interior landings permute among house entries,
    /// exterior doorsteps among exits); `random` is treated as `none`.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    house_doors: DropArg,
    /// Number of random starting items the new game begins with (`0` = leave the
    /// vanilla Healing Leaf ×5 untouched). Each is a distinct random consumable
    /// with a small random count. The random fill shares the seed's capacity
    /// (7 slots, or 5 with `--all-warps`) with the convenience-item toggles, and
    /// takes whatever they leave — so it adds on top of them rather than being
    /// crowded out. `legaia-rando starting-items` shows the current contents.
    #[arg(long, default_value_t = 0)]
    starting_items: usize,
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
    door_of_wind: Option<u8>,
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
    incense: Option<u8>,
    /// Seed the Speed Chain accessory (always act first in battle) into the new
    /// game's starting bag. Pass `--speed-chain` for the default (1) or
    /// `--speed-chain N` for N (1..=99). Additive like `--door-of-wind`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    speed_chain: Option<u8>,
    /// Seed the Chicken Heart accessory (always flee from battle) into the
    /// starting bag. `--chicken-heart` for the default (1) or `--chicken-heart N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    chicken_heart: Option<u8>,
    /// Seed the Good Luck Bell accessory (raises the item-drop rate) into the
    /// starting bag. `--good-luck-bell` for the default (1) or `--good-luck-bell N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    good_luck_bell: Option<u8>,
    /// Unlock every Door-of-Wind warp destination from the start (preset the
    /// "visited towns" story-flag bitmask). Lets Door of Wind teleport to any
    /// town immediately. Costs part of the starting-seed budget, so it caps
    /// `--starting-items` at 3.
    #[arg(long, default_value_t = false)]
    all_warps: bool,
    /// Re-introduce unused enemies (the Evil Bat duplicates that no formation
    /// references) into the random-encounter pool. Only takes effect with
    /// `--encounters random` (a `shuffle` can't introduce a new monster).
    #[arg(long, default_value_t = false)]
    unused_enemies: bool,
    /// Re-introduce unused items (the "Something Good" sell item and the unnamed
    /// Seru accessory) into the valid item pool, so a `random` drop / chest /
    /// steal fill can hand them out. Only affects the `random` modes.
    #[arg(long, default_value_t = false)]
    unused_items: bool,
    /// Comma-separated item ids (decimal or `0xHH`) to keep in their original
    /// chests, never randomized — and dropped from the random-fill pool so they
    /// can't be duplicated elsewhere. Defaults to a curated quest / key-item set
    /// (`legaia-rando chests` lists current contents to audit). Pass an empty
    /// value (`--keep-static-items ""`) to randomize everything.
    #[arg(long, value_delimiter = ',')]
    keep_static_items: Option<Vec<String>>,
    /// Write the portable PPF 3.0 patch here (defaults to `<input>.ppf`).
    #[arg(long)]
    patch: Option<PathBuf>,
    /// Also write a full patched disc-image copy here (contains Sony bytes —
    /// for local play only, never redistribute).
    #[arg(long)]
    output: Option<PathBuf>,
    /// Write a reproducibility manifest (seed + options + change summary) here.
    /// Safe to share alongside the PPF — it embeds no game bytes.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// Plan and report the run but write no files (patch / output / manifest).
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum CouplingArg {
    /// Bidirectional — you can return the way you came.
    Coupled,
    /// One-way — going back leads somewhere else.
    Decoupled,
}

impl CouplingArg {
    fn coupling(self) -> apply::DoorCoupling {
        match self {
            CouplingArg::Coupled => apply::DoorCoupling::Coupled,
            CouplingArg::Decoupled => apply::DoorCoupling::Decoupled,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum DropArg {
    /// Redistribute the existing values (drops / encounter ids).
    Shuffle,
    /// Draw each value uniformly from the valid pool.
    Random,
    /// Leave untouched.
    None,
}

#[derive(Copy, Clone, ValueEnum)]
enum ScopeArg {
    /// Each scene draws only from its own monsters (the classic behaviour).
    Scene,
    /// Each scene draws from any monster in its kingdom (Drake/Sebucus/Karisto).
    Kingdom,
    /// Each scene draws from any monster on the disc (regions fully mixed).
    World,
}

impl ScopeArg {
    fn scope(self) -> apply::EncounterScope {
        match self {
            ScopeArg::Scene => apply::EncounterScope::Scene,
            ScopeArg::Kingdom => apply::EncounterScope::Kingdom,
            ScopeArg::World => apply::EncounterScope::World,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            ScopeArg::Scene => "scene",
            ScopeArg::Kingdom => "kingdom",
            ScopeArg::World => "world",
        }
    }
}

/// Lowercase name of a mode for the manifest (valid-TOML string value).
fn mode_str(mode: DropMode) -> &'static str {
    match mode {
        DropMode::Shuffle => "shuffle",
        DropMode::Random => "random",
    }
}

impl DropArg {
    fn mode(self) -> Option<DropMode> {
        match self {
            DropArg::Shuffle => Some(DropMode::Shuffle),
            DropArg::Random => Some(DropMode::Random),
            DropArg::None => None,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Drops { input } => cmd_drops(&input),
        Cmd::Chests { input } => cmd_chests(&input),
        Cmd::Steals { input } => cmd_steals(&input),
        Cmd::Arts { input } => cmd_arts(&input),
        Cmd::Doors { input } => cmd_doors(&input),
        Cmd::HouseDoors { input } => cmd_house_doors(&input),
        Cmd::StartingItems { input } => cmd_starting_items(&input),
        Cmd::Shops { input } => cmd_shops(&input),
        Cmd::Casino { input } => cmd_casino(&input),
        Cmd::MonsterStats { input } => cmd_monster_stats(&input),
        Cmd::MovePowers { input } => cmd_move_powers(&input),
        Cmd::Affinity { input } => cmd_affinity(&input),
        Cmd::SpellCosts { input } => cmd_spell_costs(&input),
        Cmd::EquipBonuses { input } => cmd_equip_bonuses(&input),
        Cmd::WeaponSpecialty { input } => cmd_weapon_specialty(&input),
        Cmd::Randomize(args) => cmd_randomize(args),
        Cmd::Verify {
            input,
            patch,
            output,
        } => cmd_verify(&input, &patch, output.as_deref()),
    }
}

/// Resolve a user seed string to a numeric seed (shared with the in-browser
/// patcher via [`legaia_rando::rng::seed_from_str`]).
fn resolve_seed(seed: &str) -> u64 {
    legaia_rando::rng::seed_from_str(seed)
}

/// Parse an item id from a decimal or `0x`-hex string (e.g. `154` or `0x9a`).
fn parse_item_id(s: &str) -> Result<u8> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        s.parse::<u8>()
    };
    parsed.with_context(|| format!("invalid item id {s:?} (expected 0..=255, decimal or 0xHH)"))
}

fn clock_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15)
}

fn load_image(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read disc image {}", path.display()))
}

fn cmd_shops(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let shops = apply::current_shops(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u8| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    for s in &shops {
        println!(
            "[entry {:>4}] {} ({} items):",
            s.entry_idx,
            s.name,
            s.items.len()
        );
        for &id in &s.items {
            println!("    {:>3} (0x{id:02x})  {}", id, nm(id));
        }
    }
    println!("{} town shop(s) on the disc", shops.len());
    Ok(())
}

fn cmd_casino(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u16| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id as u8))
            .unwrap_or("?")
            .to_string()
    };
    match apply::current_casino(&patcher)? {
        Some(ex) => {
            for (b, block) in ex.blocks.iter().enumerate() {
                println!("block {b}:");
                for r in block {
                    let gate = if r.gate == 0 {
                        String::new()
                    } else {
                        format!("  [gated 0x{:02x}]", r.gate)
                    };
                    println!("    {:<16} {:>6} coins{gate}", nm(r.item_id), r.price);
                }
            }
        }
        None => println!("casino prize table not found"),
    }
    Ok(())
}

fn cmd_monster_stats(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_rando::disc::MONSTER_ARCHIVE_ENTRY)
        .context("read monster battle_data archive")?;
    let records =
        legaia_asset::monster_archive::records(&entry).context("decode monster archive records")?;
    println!(
        "{:>3}  {:<16} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
        "id", "name", "hp", "mp", "atk", "def+", "def-", "agl", "spd"
    );
    for r in &records {
        println!(
            "{:>3}  {:<16} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5}",
            r.id,
            r.name,
            r.hp,
            r.mp,
            r.attack(),
            r.defense_high(),
            r.defense_low(),
            r.agility(),
            r.speed()
        );
    }
    println!("{} populated monster records", records.len());
    Ok(())
}

fn cmd_move_powers(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let records =
        legaia_asset::move_power::parse(&entry).context("parse move-power table (PROT 0898)")?;

    // Tag each power-table index with the spell-table name of a move id that
    // resolves to it (the move-id space is the spell-table id space).
    let map = legaia_asset::move_power::parse_id_index_map(&entry);
    let spells = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::spell_names::SpellNameTable::from_scus(&scus));
    let label = |idx: usize| -> String {
        let (Some(map), Some(spells)) = (map.as_ref(), spells.as_ref()) else {
            return String::new();
        };
        for move_id in 0u8..=0x7F {
            if legaia_asset::move_power::index_for_move_id(map, move_id) != Some(idx as u8) {
                continue;
            }
            if let Some(name) = spells.name(move_id).filter(|n| !n.is_empty()) {
                return name.to_string();
            }
        }
        String::new()
    };

    println!("{:>3}  {:>6}  example move", "idx", "power");
    for (i, r) in records.iter().enumerate() {
        println!("{:>3}  {:>6}  {}", i, r.power(), label(i));
    }
    println!("{} move-power records", records.len());
    Ok(())
}

fn cmd_affinity(input: &Path) -> Result<()> {
    use legaia_asset::element_affinity::{ELEMENT_COUNT, Element, ElementAffinity};
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let entry = patcher
        .read_entry(legaia_asset::element_affinity::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .context("read battle-action overlay entry 0898")?;
    let aff =
        ElementAffinity::parse(&entry).context("parse element-affinity matrix (PROT 0898)")?;

    print!("{:>8}", "atk\\def");
    for d in 0..ELEMENT_COUNT {
        print!(
            " {:>7}",
            Element::from_id(d as u8).map(|e| e.name()).unwrap_or("?")
        );
    }
    println!();
    for (a, row) in aff.matrix.iter().enumerate() {
        print!(
            "{:>8}",
            Element::from_id(a as u8).map(|e| e.name()).unwrap_or("?")
        );
        for cell in row {
            print!(" {:>7}", cell);
        }
        println!();
    }
    Ok(())
}

fn cmd_spell_costs(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    match apply::current_spell_costs(&patcher)? {
        Some(spells) => {
            for s in &spells {
                println!("  {:>3}  {:<16} {:>3} MP", s.id, s.name, s.mp);
            }
            println!("{} named, costed spells", spells.len());
        }
        None => println!("spell table not found"),
    }
    Ok(())
}

fn cmd_equip_bonuses(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let nm = |id: u8| {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    match apply::current_equip_bonuses(&patcher)? {
        Some(rows) => {
            // Group consecutive same-slot rows for a readable, category-first table.
            let mut cur = "";
            for r in &rows {
                if r.slot != cur {
                    cur = r.slot;
                    println!("\n[{}]", r.slot);
                }
                let [int, atk, udf, ldf, spd] = r.stats;
                let items: Vec<String> = r.items.iter().map(|&id| nm(id)).collect();
                println!(
                    "  row {:>2}  INT {:>3} ATK {:>3} UDF {:>3} LDF {:>3} SPD {:>3}  [{}]",
                    r.row,
                    int,
                    atk,
                    udf,
                    ldf,
                    spd,
                    items.join(", ")
                );
            }
            let referenced = rows.iter().filter(|r| !r.items.is_empty()).count();
            println!(
                "\n{} bonus rows ({} referenced by equipment — the randomizable population)",
                rows.len(),
                referenced
            );
        }
        None => println!("equipment stat-bonus table not found"),
    }
    Ok(())
}

fn cmd_weapon_specialty(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let cur = apply::current_specialties(&patcher)?;
    if cur.is_empty() {
        println!("player battle files not found");
        return Ok(());
    }
    println!("character  favored weapon class");
    for a in &cur {
        let note = if a.from == a.to {
            String::new()
        } else {
            format!("  (vanilla: {})", a.from)
        };
        println!("  {:<7}  {}{note}", a.character, a.to);
    }
    println!("\n--weapon-specialty permutes these three favored classes among the characters.");
    Ok(())
}

fn cmd_drops(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let drops = apply::current_drops(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let mut n = 0;
    for d in &drops {
        if d.item == 0 {
            continue;
        }
        let name = item_names
            .as_ref()
            .and_then(|t| t.name(d.item))
            .unwrap_or("?");
        println!(
            "monster {:>3}  drop item {:>3} ({:<16})  {:>3}%",
            d.monster_id, d.item, name, d.chance
        );
        n += 1;
    }
    println!("{n} monsters have a drop (of {} slots)", drops.len());
    Ok(())
}

fn cmd_doors(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let doors = apply::current_doors(&patcher)?;
    let mut cur = String::new();
    let mut scenes = 0usize;
    for d in &doors {
        if d.home_scene != cur || cur.is_empty() {
            cur = d.home_scene.clone();
            scenes += 1;
            println!("[{:>4}] {}", d.entry_idx, d.home_scene);
        }
        println!(
            "    -> {:<10} (index {:>4})  entry=({:#04x},{:#04x}) dir={:#04x}  @0x{:x}",
            d.dest_scene, d.index, d.entry_x, d.entry_z, d.dir, d.op_pc
        );
    }
    println!("\n{} doors across {scenes} scenes", doors.len());
    Ok(())
}

fn cmd_house_doors(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let sites = apply::current_house_doors(&patcher)?;
    let cdname = legaia_iso::iso9660::read_file_in_image(patcher.image(), "CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| legaia_prot::cdname::parse_str(&s).ok());
    let scene_of = |idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for(m, idx as u32))
            .unwrap_or("?")
            .to_string()
    };
    let mut cur_entry = usize::MAX;
    let mut scenes = 0usize;
    for (idx, tx, tz) in &sites {
        if *idx != cur_entry {
            cur_entry = *idx;
            scenes += 1;
            println!("[{idx:>4}] {}", scene_of(*idx));
        }
        println!("    door warp -> tile ({tx:>3}, {tz:>3})");
    }
    println!(
        "\n{} intra-town door-warp targets across {scenes} scenes",
        sites.len()
    );
    Ok(())
}

fn cmd_chests(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let chests = apply::current_chests(&patcher)?;

    // Resolve item ids -> names (SCUS table) and PROT-entry -> scene name
    // (CDNAME.TXT), both off the user's own disc. Purely for legibility.
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let cdname = legaia_iso::iso9660::read_file_in_image(patcher.image(), "CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| legaia_prot::cdname::parse_str(&s).ok());
    let scene_of = |entry_idx: usize| -> String {
        cdname
            .as_ref()
            .and_then(|m| legaia_prot::cdname::block_for(m, entry_idx as u32))
            .unwrap_or("?")
            .to_string()
    };

    // Group consecutive chests by scene for a readable table.
    let mut last_entry: Option<usize> = None;
    let mut per_item: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for c in &chests {
        if last_entry != Some(c.entry_idx) {
            println!("\n[entry {:>4}  {}]", c.entry_idx, scene_of(c.entry_idx));
            last_entry = Some(c.entry_idx);
        }
        println!(
            "  item {:>3} (0x{:02x})  {}",
            c.item,
            c.item,
            name_of(c.item)
        );
        *per_item.entry(c.item).or_default() += 1;
    }

    println!(
        "\n{} chest give-item sites across {} scenes, {} distinct items.",
        chests.len(),
        chests
            .iter()
            .map(|c| c.entry_idx)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        per_item.len(),
    );
    println!("\nItem multiset (id  count  name):");
    for (id, count) in &per_item {
        println!(
            "  {:>3} (0x{:02x})  x{:<3}  {}",
            id,
            id,
            count,
            name_of(*id)
        );
    }
    Ok(())
}

fn cmd_arts(input: &Path) -> Result<()> {
    use legaia_art::queue::Character;
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .context("read SCUS_942.54")?;
    let entries =
        legaia_art::arts_table::parse_from_scus(&scus).context("parse arts-name table")?;
    let mut regular = 0usize;
    for ch in Character::all() {
        println!("{}:", ch.name());
        for e in entries.iter().filter(|e| e.character == ch) {
            let combo = legaia_rando::arts::pretty_combo(&e.commands);
            let tag = if e.is_miracle {
                "  [Miracle, not randomized]"
            } else {
                ""
            };
            println!(
                "  {:>2}  ap{:>3}  {:<11}  {}{}",
                e.index,
                e.ap,
                if combo.is_empty() { "-".into() } else { combo },
                e.name,
                tag
            );
            if !e.is_miracle {
                regular += 1;
            }
        }
    }
    println!(
        "\n{} arts total, {} regular arts the randomizer reassigns (3 Miracle arts left untouched).",
        entries.len(),
        regular
    );
    Ok(())
}

fn cmd_steals(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let steals = apply::current_steals(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let mut per_item: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    for s in &steals {
        println!(
            "monster {:>3}  steal item {:>3} (0x{:02x}, {:<16})  {:>3}%",
            s.monster_id,
            s.item,
            s.item,
            name_of(s.item),
            s.chance
        );
        *per_item.entry(s.item).or_default() += 1;
    }
    println!(
        "\n{} monsters are stealable, {} distinct steal items.",
        steals.len(),
        per_item.len()
    );
    Ok(())
}

fn cmd_starting_items(input: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let items = apply::current_starting_items(&patcher)?;
    let item_names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
    let name_of = |id: u8| -> String {
        item_names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };
    let all_warps = apply::current_all_warps(&patcher)?;
    if items.is_empty() {
        println!("The new game starts with an empty inventory.");
    } else {
        println!("New game starting inventory:");
        for (id, count) in &items {
            println!(
                "  {:>3} x item {:>3} (0x{:02x}, {})",
                count,
                id,
                id,
                name_of(*id)
            );
        }
        println!(
            "\n{} slot(s) seeded (the randomizer can set up to {}).",
            items.len(),
            legaia_rando::starting_items::MAX_STARTING_ITEMS
        );
    }
    println!(
        "Door-of-Wind all-warps preset: {}",
        if all_warps { "ON" } else { "off" }
    );
    Ok(())
}

fn cmd_randomize(args: RandomizeArgs) -> Result<()> {
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
    // (the unnamed accessory in particular is otherwise excluded — it has no
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

    if args.equipment_drops {
        // Equipment drops own the single drop slot, so this replaces (not
        // augments) the normal `--drops` pass.
        if mode.is_some() {
            println!("note: --equipment-drops overrides --drops (both write the one drop slot)");
        }
        let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .context("SCUS_942.54 not found in disc image (needed for --equipment-drops)")?;
        let equip_pool =
            legaia_rando::equipment::equipment_pool(&scus).context("build equipment pool")?;
        let (plan, report) = apply::randomize_equipment_drops(&mut patcher, &equip_pool, seed)?;
        println!(
            "equipment-drops: {} of {} monsters now drop rare equipment ({} gear ids in pool)",
            report.changed,
            plan.len(),
            equip_pool.len()
        );
        manifest.push("drops = \"equipment\"".to_string());
        manifest.push(format!("equipment_pool = {}", equip_pool.len()));
        manifest.push(format!(
            "equipment_drops_changed = {}  # of {} monsters",
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
    } else if let Some(mode) = mode {
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

    if let Some(enc_mode) = enc_mode {
        let scope = args.encounter_scope.scope();
        let report = apply::randomize_encounters_scoped(
            &mut patcher,
            seed,
            enc_mode,
            scope,
            unused_enemies,
        )?;
        println!(
            "encounters: {} scenes rewritten, {} ids changed ({} {})",
            report.scenes_changed,
            report.ids_changed,
            args.encounter_scope.as_str(),
            mode_str(enc_mode)
        );
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
        // Resolve the keep-static set: the curated default, or the user's
        // explicit (possibly empty) override.
        let keep_static: std::collections::BTreeSet<u8> = match &args.keep_static_items {
            None => legaia_rando::items::DEFAULT_STATIC_CHEST_ITEMS
                .iter()
                .copied()
                .collect(),
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
    };
    if seed_opts.is_active() {
        let report = apply::randomize_starting_items(&mut patcher, seed, &seed_opts)?;
        let names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
        let summary: Vec<String> = report
            .items
            .iter()
            .map(|(id, count)| {
                let nm = names.as_ref().and_then(|t| t.name(*id)).unwrap_or("?");
                format!("{count}x {nm}")
            })
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
    } else {
        println!("starting-items: untouched (vanilla Healing Leaf x5)");
        manifest.push("starting_items = 0".to_string());
        manifest.push("all_warps = false".to_string());
    }

    // Diff original vs patched -> PPF.
    let patched = patcher.into_image();
    if patched.len() != original.len() {
        bail!("patched image changed size — refusing to emit (all edits must be same-size)");
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
            "patched image: {} (contains Sony bytes — do not redistribute)",
            out.display()
        );
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
fn cmd_verify(input: &Path, patch: &Path, output: Option<&Path>) -> Result<()> {
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
            "patched image: {} (contains Sony bytes — do not redistribute)",
            out.display()
        );
    }
    Ok(())
}

/// `<stem>.<ext>` next to the input path (e.g. `disc.bin` -> `disc.ppf`).
fn with_extension(input: &Path, ext: &str) -> PathBuf {
    let mut p = input.to_path_buf();
    p.set_extension(ext);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_resolution_is_stable_and_parses_numbers() {
        // Numbers are used directly (decimal + hex).
        assert_eq!(resolve_seed("42"), 42);
        assert_eq!(resolve_seed("0x1F"), 0x1F);
        assert_eq!(resolve_seed("0XFF"), 0xFF);
        // A non-numeric string hashes stably (reproducibility contract) and the
        // same string always maps to the same seed.
        let a = resolve_seed("my cool run");
        assert_eq!(a, resolve_seed("my cool run"));
        assert_ne!(a, resolve_seed("my other run"));
        // A string that isn't a bare number doesn't collide with the number path.
        assert_ne!(resolve_seed("42x"), 42);
    }

    #[test]
    fn mode_str_is_lowercase() {
        assert_eq!(mode_str(DropMode::Shuffle), "shuffle");
        assert_eq!(mode_str(DropMode::Random), "random");
    }
}
