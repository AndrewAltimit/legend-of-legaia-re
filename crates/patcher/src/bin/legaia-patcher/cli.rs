//! Command-line interface: the `clap` argument structs, the value-enums that
//! back the randomizer options, and the small conversions from those enums into
//! the `legaia_patcher::apply` mode types.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use legaia_asset::battle_texture_catalog::BattleTextureSlot;

use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;

use crate::util::{
    parse_arts_ap_cost, parse_arts_ap_grant, parse_arts_power, parse_attack_count_scale,
    parse_exp_scale, parse_item_spec, parse_location_rename, parse_prize_price,
    parse_seru_catch_rate, parse_stat_scale,
};

#[derive(Parser)]
#[command(
    name = "legaia-patcher",
    version,
    about = "Legend of Legaia disc patcher: randomizer, translation packs, manual record edits (operates on a user-supplied disc)"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

// `Randomize` carries the whole option surface (one field per feature), so it
// dwarfs the read-only inspection subcommands. Boxing it would only move the
// allocation - clap parses exactly one variant per process.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Plan a randomization from a seed and write a PPF patch (and optionally a
    /// patched disc image copy).
    Randomize(RandomizeArgs),
    /// Read-only: list every monster's current item drop.
    Drops {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every treasure chest the randomizer would touch, grouped
    /// by scene, with the item each currently gives. Use this to audit which
    /// items would change (e.g. to spot quest items that should stay static).
    Chests {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every monster's current steal item (Evil God Icon),
    /// with its steal chance, from the static `SCUS_942.54` steal table.
    Steals {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every Tactical Art's current button combo, grouped by
    /// character, from the static `SCUS_942.54` arts-name table.
    Arts {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every scene-transition door/exit the randomizer can
    /// touch, grouped by the scene it lives in, with the destination each
    /// currently leads to.
    Doors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the intra-town (house / interior) door-warp target
    /// tiles the house-door shuffle would touch, grouped by scene - the
    /// cross-context player MOVE_TOs in named partition-0 door records (NPC /
    /// cutscene movement is excluded by construction).
    HouseDoors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the `.MAP` kind-0 intra-scene teleports (the map-data
    /// door class most house exits belong to - no script, no MAN record),
    /// grouped by scene, with each record's walk-component class. The
    /// population `--house-doors shuffle` rewires alongside the script warps.
    MapDoors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show the new game's current starting inventory (the
    /// `(item, count)` slots a New Game begins with - vanilla is Healing Leaf
    /// ×5).
    StartingItems {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every town-merchant shop and what it sells, grouped by
    /// scene, with item names.
    Shops {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the casino prize-exchange prizes (item, coin price,
    /// progression gate).
    Casino {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the fishing point-exchange prizes per venue (Buma /
    /// Vidna) with their item and fishing-point price - the population the
    /// `--fishing-price` editor changes.
    Fishing {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: print the Delilas-dome SCUS-side injection as a
    /// `LEGAIA_POKES` list for the PCSX-Redux probes (cave routines, stream
    /// hooks, PRG ERR print-gate patch, plus the course-unlock flag byte). Library
    /// save states predate the patched disc, so the probes must RAM-install
    /// the always-resident SCUS half; the overlay halves ride the `--iso`.
    DelilasPokes {
        /// Also emit the custom-items SCUS-side writes (item records,
        /// descriptors, jump-table words, cave routines).
        #[arg(long)]
        custom_items: bool,
    },
    /// Read-only: show the Earth Egg coin threshold (the Sol Tower Prize Counter
    /// exchange) - the value the `--earth-egg-price` editor changes.
    EarthEgg {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the world-map location / landmark names (index + name)
    /// - the slots the `--rename-location` editor changes.
    Locations {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every monster's current combat stats (HP / MP / ATK /
    /// UDF / LDF / INT / SPD) from the `battle_data` archive - the population
    /// the `--monster-stats` randomizer redistributes.
    MonsterStats {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Dump one monster's LZS-decoded `battle_data` block (PROT entry 867) to
    /// a file, or re-pack an edited block onto a copy of the disc - the manual
    /// monster-edit loop (stats / element / name) with no slot-offset or LZS
    /// math. `monster-stats` lists the 1-based ids; the decoded record layout
    /// is pinned in `docs/subsystems/battle.md` (element byte at `+0x1D`).
    MonsterBlock {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references). Never
        /// modified.
        #[arg(long)]
        input: PathBuf,
        /// 1-based monster id.
        #[arg(long)]
        id: u16,
        /// Write the decoded block (stat record head + name string + mesh +
        /// animations) here for editing.
        #[arg(long)]
        dump: Option<PathBuf>,
        /// Re-pack this edited block into the monster's slot on a copy of the
        /// disc (same-size in-place write, EDC/ECC re-encoded). Requires
        /// `--output` and/or `--patch`.
        #[arg(long)]
        write: Option<PathBuf>,
        /// Write the patched image here (contains Sony bytes - local play
        /// only, never redistribute).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write a portable PPF 3.0 patch here (safe to share).
        #[arg(long)]
        patch: Option<PathBuf>,
    },
    /// Read-only: list the special-attack move-power table (the 44 power values
    /// the `--move-power` randomizer redistributes), each tagged with the
    /// spell-table name of a move id that resolves to it.
    MovePowers {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: print the 8×8 element-affinity matrix (rows = attacking
    /// element, columns = defending element; each cell a damage-scale percent)
    /// the `--element-affinity` randomizer redistributes.
    Affinity {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every named spell's current MP cost from the SCUS spell
    /// table - the population the `--spell-cost` randomizer redistributes.
    SpellCosts {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the equipment stat-bonus table (`DAT_80074F68`), grouped
    /// by slot category, with each row's stats and the items that reference it -
    /// the population the `--equip-bonus` randomizer redistributes.
    EquipBonuses {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show each character's current favored weapon class (read from
    /// the player battle files) - what the `--weapon-specialty` randomizer
    /// permutes.
    WeaponSpecialty {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Apply a PPF patch to a copy of a disc and confirm it applies cleanly
    /// (records applied, the result still parses). Use this to check that a
    /// shared patch + seed match your own disc before playing.
    Verify {
        /// Path to the user's retail disc image the patch targets (`.bin`,
        /// Mode 2/2352; a `.cue` is accepted and resolved to its `.bin`).
        #[arg(long)]
        input: PathBuf,
        /// The PPF 3.0 patch to apply.
        #[arg(long)]
        patch: PathBuf,
        /// Optionally write the patched image here (for local play only).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Proceed even when the disc is not the USA build (SCUS-94254) the
        /// randomizer's patches target. A patch built against the USA disc
        /// "applies" to a PAL disc but produces a corrupt hybrid - only pass
        /// this if you know the patch was built for this exact disc.
        #[arg(long, default_value_t = false)]
        allow_region_mismatch: bool,
    },
    /// Read-only: catalog every texture on the disc with its replacement
    /// coordinates - the raw tier (uncompressed, always replaceable in
    /// place), the LZS tier (inside a compressed section, replaceable when
    /// the edited section recompresses into its footprint), and the battle
    /// tier (the party's in-battle character art, which is not a TIM at all
    /// and is replaceable within its record's slot footprint).
    TimList {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// Only list textures owned by this PROT entry.
        #[arg(long)]
        entry: Option<u32>,
        /// Which tier to list.
        #[arg(long, value_enum, default_value_t = TimTierArg::All)]
        tier: TimTierArg,
    },
    /// Decode one texture to a PNG for editing (the `tim-replace` on-ramp).
    /// Coordinates come from `tim-list`.
    TimExport {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// Owning PROT entry. Omit for a `gap`-owned texture (then --offset
        /// is the flat PROT.DAT byte offset `tim-list` shows).
        #[arg(long)]
        entry: Option<u32>,
        /// Byte offset of the TIM (decimal or 0xHEX): within the entry, or
        /// within the decoded section with --lzs-section. Not used by the
        /// battle tier, which addresses by --battle-slot instead.
        #[arg(long, value_parser = parse_u64_flexible)]
        offset: Option<u64>,
        /// LZS section index for a compressed-tier texture.
        #[arg(long)]
        lzs_section: Option<u32>,
        /// Battle-tier selector: a player-file record index, or `header0` /
        /// `header1`. Needs --entry (863..866); `tim-list --tier battle`
        /// prints both columns.
        #[arg(long, value_parser = parse_battle_slot)]
        battle_slot: Option<BattleTextureSlot>,
        /// Palette index to decode with (multi-palette textures only).
        #[arg(long, default_value_t = 0)]
        clut: usize,
        /// Where to write the PNG.
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Replace a texture with an edited PNG: same dimensions / bpp / CLUT
    /// layout enforced, VRAM placement preserved, same-size in-place write
    /// with every touched sector's EDC/ECC re-encoded. Alpha maps to the PSX
    /// STP bit (0 = transparent, 1..254 = semi-transparent, 255 = opaque).
    TimReplace {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references). Never
        /// modified.
        #[arg(long)]
        input: PathBuf,
        /// Owning PROT entry. Omit for a `gap`-owned texture (then --offset
        /// is the flat PROT.DAT byte offset `tim-list` shows).
        #[arg(long)]
        entry: Option<u32>,
        /// Byte offset of the TIM (decimal or 0xHEX): within the entry, or
        /// within the decoded section with --lzs-section. Not used by the
        /// battle tier, which addresses by --battle-slot instead.
        #[arg(long, value_parser = parse_u64_flexible)]
        offset: Option<u64>,
        /// LZS section index for a compressed-tier texture.
        #[arg(long)]
        lzs_section: Option<u32>,
        /// Battle-tier selector: a player-file record index, or `header0` /
        /// `header1`. Needs --entry (863..866).
        #[arg(long, value_parser = parse_battle_slot)]
        battle_slot: Option<BattleTextureSlot>,
        /// Battle tier only: which palette of the block to encode against.
        /// The other palettes of the same block stay byte-identical.
        #[arg(long, default_value_t = 0)]
        clut: usize,
        /// The replacement image (PNG, any color type; must match the
        /// original texture's pixel dimensions exactly).
        #[arg(long)]
        png: PathBuf,
        /// Fold excess colors to their nearest palette color instead of
        /// failing when the image holds more distinct colors than the
        /// texture's palette (4/8 bpp only).
        #[arg(long, default_value_t = false)]
        quantize: bool,
        /// Write the patched image here (contains Sony bytes - local play
        /// only, never redistribute).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write a portable PPF 3.0 patch here (safe to share).
        #[arg(long)]
        patch: Option<PathBuf>,
        /// Validate the replacement (dimensions, colors, LZS fit) without
        /// writing anything.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Read-only: list the save-slot portraits (the memory-card block icons
    /// and the save UI's per-character faces) with their slot numbers.
    SaveIconList {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Decode one save-slot portrait to a 16x16 PNG for editing.
    SaveIconExport {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// Slot to export, `0`-based. Save number `n` uses slot `n - 1`.
        #[arg(long)]
        slot: usize,
        /// Where to write the PNG.
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Replace one save-slot portrait with an edited 16x16 PNG. Only that
    /// tile's 16 pixel runs and its own 16-colour palette change; the other
    /// portraits stay byte-identical.
    SaveIconReplace {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references). Never
        /// modified.
        #[arg(long)]
        input: PathBuf,
        /// Slot to replace, `0`-based. Save number `n` uses slot `n - 1`.
        #[arg(long)]
        slot: usize,
        /// The replacement portrait (PNG, any color type, exactly 16x16).
        #[arg(long)]
        png: PathBuf,
        /// Fold excess colors to their nearest kept color instead of failing
        /// when the image holds more than 16 distinct 15-bit colors.
        #[arg(long, default_value_t = false)]
        quantize: bool,
        /// Write the patched image here (contains Sony bytes - local play
        /// only, never redistribute).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write a portable PPF 3.0 patch here (safe to share).
        #[arg(long)]
        patch: Option<PathBuf>,
        /// Validate the replacement without writing anything.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Translation / language-pack tools: export the disc's text to an
    /// editable YAML pack, generate per-language skeletons, check coverage,
    /// and import a filled pack back onto a disc copy.
    Translate {
        #[command(subcommand)]
        cmd: TranslateCmd,
    },
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum TimTierArg {
    /// Uncompressed TIMs (always replaceable in place).
    Raw,
    /// TIMs inside LZS-compressed sections (replaceable when they re-fit).
    Lzs,
    /// The headerless 4bpp party battle art in the player files
    /// (PROT 863..866). Not TIMs at all - no magic, no header - so no
    /// magic scan can reach them.
    Battle,
    /// Every tier.
    All,
}

/// Parse a battle-texture slot selector: a record index, or `header0` /
/// `header1` for the two blocks the player-file header points at.
pub(crate) fn parse_battle_slot(s: &str) -> Result<BattleTextureSlot, String> {
    s.parse()
}

/// Parse a decimal or `0x`-prefixed hexadecimal u64 (for byte offsets).
pub(crate) fn parse_u64_flexible(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    };
    parsed.map_err(|_| format!("not a number (decimal or 0xHEX): {s:?}"))
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
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
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
        /// Dry-run the pack against this disc image (`.bin`, Mode 2/2352; a
        /// `.cue` is accepted and resolved to its `.bin`).
        #[arg(long)]
        input: Option<PathBuf>,
        /// Print every skipped / over-budget entry individually instead of
        /// the default per-reason summary.
        #[arg(long, default_value_t = false)]
        verbose: bool,
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
        /// ASCII-fold the accented glyphs the NTSC font lacks (`Epee` for
        /// `Épée`). Without it the lifted text keeps the PAL accent bytes,
        /// which render blank until the font atlas is patched.
        #[arg(long)]
        fold_accents: bool,
    },
    /// Measure how much of an official localization fits the USA target under
    /// the per-string vs per-MAN (generalized rewriter) budget, and how many
    /// scene MANs remain sector-crossers. Counts only - no text - so it is safe
    /// to run and log.
    FitReport {
        /// The official-localization disc (PAL SCES build).
        #[arg(long)]
        from: PathBuf,
        /// The USA target disc.
        #[arg(long)]
        target: PathBuf,
    },
    /// Apply a filled pack to a copy of a disc. Untranslated entries are
    /// left byte-identical; every write is same-size in place and each
    /// touched sector's EDC/ECC is re-encoded.
    Import {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
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
        /// Allow a whole-sector **disc relayout**: scene MANs whose full-length
        /// dialog overflows their compressed footprint gain `+N` sectors (the
        /// PROT entry grows and the disc is relaid out) so the dialog imports
        /// byte-faithfully instead of being abbreviated. Grows the image.
        #[arg(long)]
        allow_relayout: bool,
        /// Print every skipped entry individually instead of the default
        /// per-reason summary.
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
}

#[derive(Parser)]
pub(crate) struct RandomizeArgs {
    /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
    /// is accepted and resolved to the `.bin` it references).
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
    #[arg(long, default_value_t = legaia_patcher::bonus_drop::DEFAULT_CHANCE_PCT)]
    pub(crate) equipment_drop_chance: u8,
    /// Bank a slice of the formation's experience into the party whenever they
    /// **successfully run away** (a same-size code hook into the battle-action
    /// escape teardown). Vanilla awards nothing for fleeing; this credits
    /// `--flee-exp-pct`% of the fled fight's EXP to every party member.
    #[arg(long, default_value_t = false)]
    pub(crate) flee_exp: bool,
    /// Percentage of the formation's experience banked on a successful escape
    /// (only with `--flee-exp`).
    #[arg(long, default_value_t = legaia_patcher::flee_exp::DEFAULT_PCT)]
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
    #[arg(long, default_value_t = legaia_patcher::enemy_ally::DEFAULT_PCT)]
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
    /// **Delilas Challenge**: a fourth option on the Muscle Dome enrollment
    /// menu - a brand-new 2-round arena course: Che & Lu Delilas together
    /// (1v2), then Gi (1v1). The double-team fits the battle heap by
    /// streaming slim clones (their generic castable spells trimmed) from
    /// two unreachable archive slots - the real 163/164 records are never
    /// modified, and the fight keeps the signature specials, the
    /// attack-attack-special AI, and the real names. A full clear pays 5000
    /// coins. Unlocks after the Koru event in Nivora Ravine (the `nilboa2`
    /// story flag). Losing a round returns to the Sol venue by the dome's
    /// own design - no game over. A `koin1` script edit plus a small
    /// arena/SCUS code injection.
    #[arg(long, default_value_t = false)]
    pub(crate) delilas_challenge: bool,
    /// **Custom items**: inject three brand-new items into cut item slots -
    /// Nature's Elixir (full HP+MP restore), the Ra-Seru Tear (a free cast
    /// of the user's own Ra-Seru summon), and the Fury Bloom (party-wide
    /// Fury Boost). Standalone: with a `random` drop / chest / steal mode
    /// they join the fill pool, and with `--delilas-challenge` they replace
    /// the Honey as the course's full-clear reward.
    #[arg(long, default_value_t = false)]
    pub(crate) custom_items: bool,
    /// Per-battle percentage chance a capturable enemy is shiny (only with
    /// `--shiny-seru`).
    #[arg(long, default_value_t = legaia_patcher::shiny_seru::DEFAULT_PCT)]
    pub(crate) shiny_pct: u8,
    /// **Jewel fix**: make the boss cinematic casts respect elemental guards.
    /// In retail, Xain's Bloody Horns / Terio Punch (+ Bull Charge), Cort's
    /// Guilty Cross, and the Delilas trio's Blazing Slash /
    /// Megaton Press / Plasma Strike call the resist-ladder-bypassing damage
    /// wrapper (`FUN_801DD6B4`, finisher `param_5 = 1`), so Earth/other
    /// Jewels, elemental guards and All Guard never apply to them despite the
    /// caster's element being read by the affinity scale. This retargets the
    /// thirteen damage `jal`s in the six streamed cast modules to the
    /// guard-respecting wrapper `FUN_801DD4B0` (same-size word edits in PROT
    /// entries 944 / 952 / 953 / 958 / 959 / 960); casts that already respect
    /// guards (incl. Neo Star Slash, which shares Plasma Strike's module) are
    /// untouched.
    #[arg(long, default_value_t = false)]
    pub(crate) jewel_fix: bool,
    /// Fix the **attack-approach softlock** (the "endless camera orbit"): a
    /// monster approaching an out-of-reach target waits in a range poll with
    /// no timeout while its approach animation drives the movement - and
    /// when that animation dies mid-approach (a summon immediately before
    /// the melee; caught live and reproduced on the Gaza rematch) nothing
    /// re-stages it and the battle waits forever. A nine-word rewrite of the
    /// poll's redundant facing recompute re-stages the dead animation
    /// through the game's own staging state, so the monster simply resumes
    /// walking. Healthy approaches, party attacks, and in-range attacks
    /// behave byte-for-byte like retail.
    #[arg(long, default_value_t = false)]
    pub(crate) approach_softlock_fix: bool,
    /// Set how much **AP the Spirit command charges** into the battle AP
    /// gauge (retail 32). `0` makes Spirit a pure defensive stance (guard
    /// boost, no AP); `100` fills the whole gauge in one press; a **negative**
    /// value makes Spirit *drain* the gauge instead, floored at zero.
    /// Rewrites the battle engine's per-action AP accrual for the Spirit
    /// category plus the three gauge-widget ramp targets that mirror it -
    /// four same-size word edits in the battle overlay (PROT 898). A negative
    /// value additionally makes the accrual read signed and neutralizes the
    /// AP-Boost accessory arms (they read the accrual unsigned), so an
    /// AP-Boost accessory is inert while the setting is negative.
    #[arg(long, value_name = "AP", allow_negative_numbers = true,
          value_parser = clap::value_parser!(i16).range(-100..=100))]
    pub(crate) spirit_ap: Option<i16>,
    /// Set how much **AP taking damage grants**, as AP per 100% of max HP
    /// lost (retail 100: a hit that takes your whole HP bar fills the
    /// 100-point gauge). `0` means damage never feeds the gauge; `200`
    /// doubles the fill rate; a **negative** value makes being hit *drain*
    /// the gauge instead, floored at zero. Rewrites the battle damage
    /// finisher's scale in the battle overlay (PROT 898); as with
    /// `--spirit-ap`, a negative value neutralizes the AP-Boost accessory
    /// arms.
    #[arg(long, value_name = "AP", allow_negative_numbers = true,
          value_parser = clap::value_parser!(i16).range(-200..=200))]
    pub(crate) damage_ap: Option<i16>,
    /// Set the **fishing-exchange price** of one or more prizes. Comma- or
    /// repeat-separated `ITEM=POINTS` entries (`--fishing-price 0x6F=500` sets
    /// the Water Egg to 500 fishing points; ids in decimal or `0xHH`). The
    /// price is both the point cost and the "only appears once you can afford
    /// it" gate, so lowering it also makes the prize show up sooner. Applies to
    /// every venue (Buma / Vidna) row granting that item. `legaia-patcher
    /// fishing` lists the current prizes and prices.
    #[arg(long, value_name = "ITEM=POINTS", value_delimiter = ',', value_parser = parse_prize_price)]
    pub(crate) fishing_price: Vec<(u8, u32)>,
    /// Set the **Earth Egg coin threshold** - the casino-coin count the Sol
    /// Tower "Prize Counter" requires before it offers to exchange coins for the
    /// Earth Ra-Seru Egg (retail 100000). This is a bespoke scripted exchange,
    /// *not* a row in the casino prize table, so the shop / casino editors can't
    /// reach it. VALUE is the coins required; the game debits exactly that many
    /// on purchase (gate = value - 1, debit = value, matching retail). Range
    /// 1..=8388608. `legaia-patcher earth-egg` shows the current value.
    #[arg(long, value_name = "VALUE")]
    pub(crate) earth_egg_price: Option<u32>,
    /// **Rebalance a Tactical Art's damage** ("arts power-down"). Comma- or
    /// repeat-separated `COMBO=VALUE` entries, targeting an art by its input
    /// combo (`L/R/D/U`, e.g. `--arts-power RDLDL=0x16`). `VALUE` is a
    /// power-encoding byte: `0x0C..=0x1F` (a defence facet + one of the
    /// multipliers 12/18/20/22/28; higher = stronger, so a *lower* value powers
    /// the art down), or `0` to disable that art's hits. Every active per-strike
    /// power byte of the matched art is set to `VALUE` (hit count preserved).
    /// `legaia-patcher arts` lists every art's combo and current power tiers.
    #[arg(long, value_name = "COMBO=VALUE", value_delimiter = ',', value_parser = parse_arts_power)]
    pub(crate) arts_power: Vec<(Vec<legaia_art::queue::Command>, u8)>,
    /// **Make a Tactical Art grant AP instead of costing it** ("arts AP-grant").
    /// Comma- or repeat-separated `[CHARACTER:]COMBO=AMOUNT` entries, targeting
    /// an art by its input combo (`L/R/D/U`, e.g. `--arts-ap-grant RDLDL=10` or
    /// `--arts-ap-grant Vahn:RDLDL=10`). `AMOUNT` is the AP (Spirit) granted per
    /// use (1..=100); the art becomes castable at any AP level and *adds* that
    /// much (clamped at 100) rather than paying a cost. A same-size code hook
    /// into the party arts queue-builder (PROT 0898) plus routines + a
    /// per-(character, row) config table in verified-dead SCUS regions. Without
    /// a `CHARACTER:` prefix every character holding that combo is targeted -
    /// each in its own cell, so nothing spills onto another character's art.
    /// The art's menu AP number is rewritten to match (a grant shows `0`).
    /// **Mutually exclusive with `--shiny-seru`** (same arena bytes).
    #[arg(long, value_name = "COMBO=AMOUNT", value_delimiter = ',', value_parser = parse_arts_ap_grant)]
    pub(crate) arts_ap_grant: Vec<legaia_patcher::arts_ap_grant::ArtApSpec>,
    /// **Set what a Tactical Art costs in AP** ("arts AP-cost"). Same
    /// `[CHARACTER:]COMBO=AMOUNT` syntax as `--arts-ap-grant`; `AMOUNT` is the
    /// flat AP (Spirit) the art charges per use (1..=100), replacing retail's
    /// computed `multiplier x command_count`. Retail stores no per-art cost, so
    /// this rides the same code hook as `--arts-ap-grant`; both the
    /// affordability gate and the charged amount follow the setting, and the
    /// art's menu AP number is rewritten to match. `0` is not available - it is
    /// the config table's "leave at retail" value, so the cheapest art is 1 AP.
    /// **Mutually exclusive with `--shiny-seru`** (same arena bytes).
    #[arg(long, value_name = "COMBO=AMOUNT", value_delimiter = ',', value_parser = parse_arts_ap_cost)]
    pub(crate) arts_ap_cost: Vec<legaia_patcher::arts_ap_grant::ArtApSpec>,
    /// **Rename a place everywhere the game shows it**: the quick-travel /
    /// Door-of-Wind list, the label drawn over the world map at its map
    /// position, and the banner shown on entering the scene (which is also the
    /// save-screen location row). Repeatable `TARGET=NAME` entries, where
    /// `TARGET` is either a landmark cell index or the place's current name
    /// (`legaia-patcher locations` lists both - the 14 places with a world-map
    /// label but no quick-travel cell, e.g. "Hunter's Spring" or "Sol Tower",
    /// are addressable only by name). Matching is exact, so renaming "Conkram"
    /// leaves "Conkram (Past)" alone. The new name is ASCII, up to 23
    /// characters. E.g. `--rename-location "3=Ancient Fire Cave"`.
    #[arg(long, value_name = "TARGET=NAME", value_parser = parse_location_rename)]
    pub(crate) rename_location: Vec<(legaia_patcher::apply::RenameTarget, String)>,
    /// Add the in-shop **Seru trading** vendor: every merchant grows a fourth
    /// Trade row opening a screen that swaps a party member's learned
    /// Seru-magic for a different one (offer reseeds on a play-time bucket,
    /// deterministic from the run's seed). Runs on real hardware - the whole
    /// screen is hand-assembled MIPS hosted in the menu overlay - and the same
    /// seed is embedded in `SCUS_942.54` for the clean-room engine's trade UI.
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
    /// How monster combat stats are reassigned (HP / MP / ATK / UDF / LDF /
    /// INT / SPD from the `battle_data` archive). `shuffle` permutes each stat
    /// column across the roster (each stat's multiset preserved, so the overall
    /// difficulty budget is kept); `random` draws each stat from that column's
    /// pool. AGL is left untouched - it gates the enemy AI's action economy
    /// rather than player-facing difficulty. `legaia-patcher monster-stats` lists
    /// the current stats.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) monster_stats: DropArg,
    /// **Scale every enemy's combat stats** by a difficulty multiplier
    /// (`0.1x..=5x` per stat, retail `1`). Three spellings, which nest: one
    /// number scales everything - `--enemy-stat-scale 2` doubles every
    /// monster's HP / MP / ATK / UDF / LDF / INT / SPD, `0.5` halves them - or
    /// a per-stat list scales only what it names, leaving the rest at retail:
    /// `--enemy-stat-scale hp=3` for spongy-but-not-lethal enemies,
    /// `--enemy-stat-scale attack=2,defense=0.5` for glass cannons. Stat keys:
    /// `hp`, `mp`, `attack`, `defense` (both halves) or `defense_high` /
    /// `defense_low` individually, `intelligence`, `speed`.
    /// Third, a `|`-separated **per-group split** gives random encounters and
    /// boss fights their own scale, each spelled either of the above ways:
    /// `--enemy-stat-scale 'regular:0.75|boss:2'` for a breezier road and
    /// harder set-pieces, `--enemy-stat-scale 'boss:hp=2'` to make only the
    /// bosses spongy. Groups are `regular` / `boss` / `all`; an unnamed group
    /// falls back to `all`, or to retail. Which monsters count as bosses is read
    /// off the disc's own encounter tables (scripted-only fights), not a list.
    /// Unlike `--monster-stats` this moves nothing between monsters - each
    /// keeps its own profile and each group shifts together, **story bosses
    /// included** (only the unwinnable-by-design Rim Elm sparring partner is
    /// pinned, since a weakened one can soft-lock the tutorial).
    /// AGL is left alone, as it gates the AI's action economy rather than
    /// difficulty, and EXP / gold / drops never move - a 5x run is harder, not
    /// richer. Seedless, and applied *after* `--monster-stats`, so the two
    /// compose. `legaia-patcher monster-stats` lists the current stats.
    #[arg(long, value_name = "MULT|STAT=MULT,...|GROUP:SCALE|...", value_parser = parse_stat_scale)]
    pub(crate) enemy_stat_scale: Option<legaia_patcher::monster_stats::ScaleProfile>,
    /// **Scale every battle's EXP payout** by a multiplier (`0.1x..=5x`,
    /// retail `1`): `--exp-scale 2` doubles every monster's base EXP reward,
    /// `--exp-scale 0.5` halves it. Edits the base-EXP halfword in each
    /// monster's record, so the victory spoils, the post-battle split among
    /// living party members, and the `--flee-exp` grant all scale together.
    /// Gold, drops and everything else stay retail. A scaled reward never
    /// drops to zero (floors at 1 EXP) and saturates at 65535. Seedless.
    #[arg(long, value_name = "MULT", value_parser = parse_exp_scale)]
    pub(crate) exp_scale: Option<legaia_patcher::monster_stats::ScalePermille>,
    /// **Override every capturable Seru's catch rate** with one flat percent
    /// (`0..=100`): the chance that a killing blow on a Seru monster absorbs
    /// its magic. Retail rates run from 80% (an early Gimard) down to 1% (the
    /// rarest late-game Seru); `--seru-catch-rate 100` makes every eligible
    /// kill absorb, `--seru-catch-rate 0` disables absorption entirely. Only
    /// the 63 capturable records are touched - the override never makes a
    /// non-Seru monster capturable, and the Ivory Book's +30-point bonus
    /// still applies on top (capped by the roll's own d100).
    #[arg(long, value_name = "PCT", value_parser = parse_seru_catch_rate)]
    pub(crate) seru_catch_rate: Option<u8>,
    /// **Scale how many hits enemies land with their standard attacks** by a
    /// multiplier (`0.1x..=5x`, retail `1`): `--enemy-attack-count 2` roughly
    /// doubles every enemy's physical strikes per turn, `0.5` halves them.
    /// Retail prices each attack in AGL and lets the per-round AGL gauge
    /// afford as many strikes as fit, so this divides each attack entry's
    /// AGL-cost byte by the multiplier while leaving AGL itself alone -
    /// composing cleanly with `--enemy-stat-scale`, which never touches AGL.
    /// Scaled costs round half up and clamp so an enemy that attacks in
    /// retail always lands **at least one** hit per attack turn (never zero),
    /// and the engine's own 15-action queue bounds the top end. Unavailable
    /// (`0xFF`) and deliberately overpriced entries are left alone, so
    /// movesets never change - only counts. Spell casts are untouched. Only
    /// the unwinnable-by-design Rim Elm sparring partner is pinned. Seedless.
    #[arg(long, value_name = "MULT", value_parser = parse_attack_count_scale)]
    pub(crate) enemy_attack_count: Option<legaia_patcher::monster_stats::ScalePermille>,
    /// How special-attack power is reassigned (the battle-action move-power
    /// table - enemy specials + Seru-magic, NOT party Tactical Arts). `shuffle`
    /// permutes the 44 power values (multiset preserved); `random` draws each
    /// from that pool. Only the power changes - each move keeps its own
    /// animation, effects, and sound. `legaia-patcher move-powers` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) move_power: DropArg,
    /// How the element-affinity matrix is reassigned (which element beats which:
    /// the 8×8 damage-scale grid). `shuffle` permutes the 64 cells (the same
    /// number of weaknesses / resistances exists, between different pairs);
    /// `random` draws each cell from that pool. Per-character element assignment
    /// is left untouched. `legaia-patcher affinity` shows the current grid.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) element_affinity: DropArg,
    /// How spell MP costs are reassigned (the SCUS spell table). `shuffle`
    /// permutes the MP costs of the named, costed spells (the cost multiset is
    /// preserved); `random` draws each from that pool. Free / internal-tier
    /// spells never gain a cost. `legaia-patcher spell-costs` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) spell_cost: DropArg,
    /// How equipment passive stat bonuses are reassigned (the SCUS bonus table).
    /// `shuffle` permutes each slot category's stat tuples (`INT/ATK/UDF/LDF/SPD`)
    /// among that category's gear (so weapon power lands on another weapon, armor
    /// on armor; the per-category budget is kept); `random` draws each from that
    /// category's pool. The equip-character mask, accessory passive, and slot type
    /// never move. `legaia-patcher equip-bonuses` lists the current table.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) equip_bonus: DropArg,
    /// How the equip-character mask is reassigned (who can wear each piece of
    /// gear - the `+6` byte of the SCUS bonus table). `shuffle` permutes the
    /// masks within each slot category (each character keeps the same count of
    /// equippable weapons / body / head / footwear, just on different items);
    /// `random` draws each row's mask from its category pool. Stat bonuses and
    /// slot type never move, so this composes with `--equip-bonus`.
    /// `legaia-patcher equip-bonuses` lists the current masks.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) equip_mask: DropArg,
    /// Reassign which weapon class each character specializes in (Vahn blades,
    /// Noa claws, Gala clubs/axes by default). Permutes the three favored
    /// families among the characters and rewrites the per-(character, weapon)
    /// arm-cost byte in the player battle files, so an off-class weapon widens
    /// the Arms command in an arts combo. The Astral Sword stays always-wide.
    /// `legaia-patcher weapon-specialty` shows the current favored class per char.
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
    /// left untouched. `legaia-patcher arts` lists current combos.
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
    /// preserved, so no rewire can strand the player). `legaia-patcher
    /// house-doors` / `map-doors` list the two populations.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) house_doors: DropArg,
    /// Number of random starting items the new game begins with (`0` = leave the
    /// vanilla Healing Leaf ×5 untouched). Each is a distinct random consumable
    /// with a small random count. The random fill shares the seed's capacity
    /// (7 slots, or 5 with `--all-warps`) with the convenience-item toggles, and
    /// takes whatever they leave - so it adds on top of them rather than being
    /// crowded out. `legaia-patcher starting-items` shows the current contents.
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
    /// Seed the Good Luck Bell accessory (Low Encounter - halves the encounter
    /// rate) into the starting bag. It only puts the item in the bag; equipping
    /// it is what applies the passive.
    /// `--good-luck-bell` for the default (1) or `--good-luck-bell N`.
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
    /// opening scene like the random fill. `legaia-patcher starting-items` shows
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
    /// 2..=14 (the XP seed is a single 16-bit immediate). `legaia-patcher
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
    /// moved or randomly placed). `legaia-patcher chests` lists current contents
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
    /// Proceed even when the disc is not the USA build (SCUS-94254). The
    /// randomizer's offsets and code hooks target the USA disc; running it
    /// against a PAL disc (SCES_019.44/.45/.46) produces a corrupt hybrid.
    #[arg(long, default_value_t = false)]
    pub(crate) allow_region_mismatch: bool,
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
