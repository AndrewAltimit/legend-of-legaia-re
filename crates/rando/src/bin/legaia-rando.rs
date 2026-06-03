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
    /// Read-only: list every scene-transition door/exit the randomizer can
    /// touch, grouped by the scene it lives in, with the destination each
    /// currently leads to.
    Doors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the intra-town (house / interior) MOVE_TO target tiles
    /// the house-door shuffle would touch, grouped by scene. NOTE: this set
    /// includes some NPC / cutscene movement, not only house-door warps.
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
    /// How monster item drops are reassigned.
    #[arg(long, value_enum, default_value_t = DropArg::Shuffle)]
    drops: DropArg,
    /// How random-encounter formations are reassigned (within each scene's own
    /// monster pool, so every monster stays scene-loaded).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    encounters: DropArg,
    /// How treasure-chest contents are reassigned (global; `random` draws from
    /// the valid item pool, `shuffle` redistributes the existing chest items).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    chests: DropArg,
    /// How per-monster steal items are reassigned (the Evil God Icon table;
    /// `shuffle` redistributes the existing steal items, `random` draws from the
    /// valid item pool — the steal *chance* is always preserved).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    steals: DropArg,
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
    /// is meaningful (a per-scene, multiset-preserving shuffle of the `0x23`
    /// MOVE_TO target tiles); `random` is treated as `none`. Experimental:
    /// the shuffled set includes some NPC / cutscene movement, not only doors.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    house_doors: DropArg,
    /// Number of random starting items the new game begins with (`0` = leave the
    /// vanilla Healing Leaf ×5 untouched). Each is a distinct random consumable
    /// with a small random count; capped at the seed region's capacity (5).
    /// `legaia-rando starting-items` shows the current contents.
    #[arg(long, default_value_t = 0)]
    starting_items: usize,
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
        Cmd::Doors { input } => cmd_doors(&input),
        Cmd::HouseDoors { input } => cmd_house_doors(&input),
        Cmd::StartingItems { input } => cmd_starting_items(&input),
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
        println!("    MOVE_TO tile ({tx:>3}, {tz:>3})");
    }
    println!(
        "\n{} intra-town MOVE_TO targets across {scenes} scenes \
         (includes some NPC / cutscene movement, not only house doors)",
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
    if items.is_empty() {
        println!("The new game starts with an empty inventory.");
        return Ok(());
    }
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
    let door_mode = args.doors.mode();

    println!("seed: {seed} (0x{seed:016X})");
    // Manifest lines accumulate the run's options + outcome for reproducibility.
    let mut manifest = vec![
        "# legaia-rando run manifest".to_string(),
        format!("seed = {seed}  # 0x{seed:016X}"),
        format!("input = {:?}", args.input.display().to_string()),
    ];

    // The valid item pool (from SCUS) is needed only by the `random` modes.
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

    if let Some(enc_mode) = enc_mode {
        let report = apply::randomize_encounters(&mut patcher, seed, enc_mode, unused_enemies)?;
        println!(
            "encounters: {} scenes rewritten, {} ids changed ({:?})",
            report.scenes_changed, report.ids_changed, enc_mode
        );
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
                "house-doors: {} of {} MOVE_TO targets shuffled across {} scenes",
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
                "house-doors: only `shuffle` is supported (random would place actors off-map); untouched"
            );
            manifest.push("house_doors = \"none\"".to_string());
        }
    } else {
        println!("house-doors: untouched");
        manifest.push("house_doors = \"none\"".to_string());
    }

    if args.starting_items > 0 {
        let report = apply::randomize_starting_items(&mut patcher, seed, args.starting_items)?;
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
            "starting-items: new game now begins with {} random item(s): {}",
            report.items_set,
            summary.join(", ")
        );
        manifest.push(format!("starting_items = {}", report.items_set));
        manifest.push(format!("starting_items_set = {:?}", report.items));
    } else {
        println!("starting-items: untouched (vanilla Healing Leaf x5)");
        manifest.push("starting_items = 0".to_string());
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
