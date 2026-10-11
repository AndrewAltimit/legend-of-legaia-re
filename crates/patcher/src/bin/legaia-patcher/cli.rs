//! Command-line interface: the `clap` argument structs, the value-enums that
//! back the randomizer options, and the small conversions from those enums into
//! the `legaia_patcher::apply` mode types.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use legaia_actor_formats::battle_texture_catalog::BattleTextureSlot;

mod randomize_args;
mod translate_args;

pub(crate) use randomize_args::{RandomizeArgs, mode_str};
pub(crate) use translate_args::TranslateCmd;

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
    /// Read-only: verify a patched image carries the current
    /// `--delilas-party` build invariants. Detects the swap (hero-named
    /// sibling monster blocks), then checks every rebuilt player battle
    /// file: no `0xFE` equipment extras survive in any equipment assembly
    /// (the Spirit-streak / foreign-pointer class), every hand is seated at
    /// its wrist, and every skeleton part carries geometry. Use this on the
    /// exact `.bin` you are about to play-test - it catches a rom built by
    /// a stale patcher (cached wasm, old server) in seconds, before an
    /// emulator session is wasted on it.
    DelilasVerify {
        /// Path to the patched disc image to verify (`.bin`, Mode 2/2352; a
        /// `.cue` is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only defect battery for a `--delilas-party` image, measured
    /// against the retail baseline: per-clip FK joint closures + posed
    /// extents vs the baseline's own numbers, an animation-stream census
    /// (base-archive lead-ins must stay retail - the battle plays them),
    /// and welded-weapon hand-radius checks. Two discs in, verdicts out;
    /// no emulator or save state needed.
    DelilasAudit {
        /// Path to the patched disc image (`.bin` / `.cue`).
        #[arg(long)]
        input: PathBuf,
        /// Path to the retail baseline disc image (`.bin` / `.cue`).
        #[arg(long)]
        baseline: PathBuf,
        /// Accept a kept welded weapon fist (a `--delilas-che-hammer`
        /// comparison build): the hand-radius battery reports the welded
        /// weapon as informational instead of a FAIL.
        #[arg(long)]
        allow_kept_hammer: bool,
    },
    /// Read-only: emit RAM pokes (`0xADDR:0xWORD` lines) for every SCUS word
    /// where a patched disc differs from a baseline disc. A save state carries
    /// the whole resident SCUS, so a probe that loads a state from one disc
    /// era onto a newer patched disc runs fresh overlay code against stale
    /// SCUS injections - a per-frame dynarec fault in the injection arena.
    /// Applying these pokes after the state load makes the resident SCUS
    /// byte-match the disc under test (the e2e harness consumes this).
    ScusPokes {
        /// The patched disc image under test.
        #[arg(long)]
        patched: PathBuf,
        /// The baseline disc (usually the retail image).
        #[arg(long)]
        baseline: PathBuf,
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
    /// the `--monster-stats` randomizer redistributes - plus its juggle
    /// window: the light-flinch clip's gate frame over its rate byte
    /// (`f8/2`) and the 60 Hz ticks a follow-up hit has to grow the juggle
    /// counter (animation data, not a stat).
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
    /// Export a monster's 3D model (mesh + texture page + baked colours) as
    /// editable OBJ+MTL+PNG, or replace it with a custom model. A replacement
    /// keeping the retail `o part_NN` group count performs every retail
    /// battle animation - the streams pose parts rigidly by index. Written
    /// back as the same same-size in-place slot edit `monster-block` uses.
    MonsterModel {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references). Never
        /// modified.
        #[arg(long)]
        input: PathBuf,
        /// 1-based monster id (`monster-stats` lists the populated ids).
        #[arg(long)]
        id: u16,
        /// Export the model here: writes `<stem>.obj`, `<stem>.mtl`,
        /// `<stem>.png`.
        #[arg(long, value_name = "STEM")]
        export: Option<PathBuf>,
        /// Replacement model (Wavefront OBJ, `o part_NN` groups, raw GTE
        /// units). Requires `--texture`, plus `--output`/`--patch` or
        /// `--dry-run`.
        #[arg(long)]
        obj: Option<PathBuf>,
        /// Replacement texture page (PNG, same dimensions as the retail
        /// page - see the exported PNG).
        #[arg(long)]
        texture: Option<PathBuf>,
        /// Permit the decoded block to grow past its retail size (the battle
        /// heap budget is tuned to retail; growth risks the loader's
        /// unchecked allocation). The 0x14000 compressed-slot cap always
        /// applies.
        #[arg(long)]
        allow_grow: bool,
        /// Validate + report sizes without writing anything.
        #[arg(long)]
        dry_run: bool,
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
    /// Read-only: list every equippable item with who can equip it and, for
    /// weapons, the Arts-gauge swing cost each character's player file carries
    /// for it - the table `--swing-cost` and `--equip-owner` edit.
    Equipment {
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
        /// Monster-tier selector: the 1-based monster id `tim-list --tier
        /// monster` prints. Needs no --entry; the archive is always 867.
        #[arg(long)]
        monster_id: Option<u16>,
        /// Palette index to decode with (multi-palette textures only). The
        /// monster tier ignores it - each texel is decoded through the
        /// palette the model reads it with. On a TIM whose per-region
        /// palettes are known (the menu / battle UI sheet) leaving it out
        /// draws every region through the palette the game uses for it.
        #[arg(long)]
        clut: Option<usize>,
        /// Draw every region through the palette the game uses for it (the
        /// default when that is known and --clut is not given).
        #[arg(long, default_value_t = false)]
        in_game: bool,
        /// What to write (TIM tiers only). `image`: the texture drawn through
        /// one view. `composite`: that image with every palette as rows of
        /// colour cells below it - edit pixels and colours in one file.
        /// `strip`: the palettes alone, one row per palette. `indexed`: an
        /// indexed PNG of the stored palette indices. `tim-replace`
        /// recognises all four.
        #[arg(long, value_enum, default_value_t = TimExportFormatArg::Image)]
        format: TimExportFormatArg,
        /// Where to write the PNG.
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Read-only: which palette the game draws each region of a
    /// multi-palette texture through (known for the menu / battle UI sheet,
    /// from the executable's widget table). Coordinates come from
    /// `tim-list`.
    TimPaletteMap {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352; a `.cue`
        /// is accepted and resolved to the `.bin` it references).
        #[arg(long)]
        input: PathBuf,
        /// Owning PROT entry. Omit for a `gap`-owned texture.
        #[arg(long)]
        entry: Option<u32>,
        /// Byte offset of the TIM (decimal or 0xHEX).
        #[arg(long, value_parser = parse_u64_flexible)]
        offset: u64,
        /// LZS section index for a compressed-tier texture.
        #[arg(long)]
        lzs_section: Option<u32>,
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
        /// Monster-tier selector: the 1-based monster id. This tier never
        /// rewrites a palette, so the replacement may only use colors the
        /// monster's own palettes already hold (or pass --quantize).
        #[arg(long)]
        monster_id: Option<u16>,
        /// Battle tier only: which palette of the block to encode against.
        /// The other palettes of the same block stay byte-identical. (TIM
        /// tiers need no flag: `tim-replace` recognises the view, composite,
        /// palette strip or indexed PNG `tim-export` wrote.)
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
pub(crate) enum TimExportFormatArg {
    /// The texture's own size, drawn through one view.
    Image,
    /// The image plus every palette as colour-cell rows below it.
    Composite,
    /// The palettes alone (edit colours without touching pixels).
    Strip,
    /// An indexed PNG of the stored palette indices.
    Indexed,
}

impl From<TimExportFormatArg> for legaia_texture_replace::texture::ExportFormat {
    fn from(a: TimExportFormatArg) -> Self {
        use legaia_texture_replace::texture::ExportFormat as F;
        match a {
            TimExportFormatArg::Image => F::Image,
            TimExportFormatArg::Composite => F::Composite,
            TimExportFormatArg::Strip => F::Strip,
            TimExportFormatArg::Indexed => F::Indexed,
        }
    }
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
    /// The headerless 4bpp battle skin every enemy and boss wears, one page
    /// per monster inside its own compressed archive slot (PROT 867). Also
    /// not TIMs, and listed by monster name + id.
    Monster,
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
