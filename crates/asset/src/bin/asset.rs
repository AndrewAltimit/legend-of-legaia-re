use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[path = "asset/actors.rs"]
mod actors;
#[path = "asset/catalogs.rs"]
mod catalogs;
#[path = "asset/common.rs"]
mod common;
#[path = "asset/dispatch.rs"]
mod dispatch;
#[path = "asset/overlay.rs"]
mod overlay;
#[path = "asset/packs.rs"]
mod packs;
#[path = "asset/stage.rs"]
mod stage;
#[path = "asset/summon.rs"]
mod summon;
#[path = "asset/tables.rs"]
mod tables;
#[path = "asset/validation.rs"]
mod validation;
#[path = "asset/worldmap.rs"]
mod worldmap;

use actors::*;
use catalogs::*;
use dispatch::*;
use overlay::*;
use packs::*;
use stage::*;
use summon::*;
use tables::*;
use validation::*;
use worldmap::*;

#[derive(Parser)]
#[command(name = "asset", about = "Legaia asset descriptor + dispatcher")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse a buffer as a player.lzs-style container header and print.
    Describe {
        input: PathBuf,
        /// Number of descriptors to read after the 8-byte meta (default 3).
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
    /// Decode one descriptor's payload (`--type-size 0xTTSSSSSS --offset 0xNN --mode lzs|raw`).
    Decode {
        input: PathBuf,
        /// `(type<<24) | size` packed into a single u32, hex-prefixed (e.g. 0x02001000).
        #[arg(long, value_parser = parse_hex_u32)]
        type_size: u32,
        /// Byte offset within the input buffer.
        #[arg(long, value_parser = parse_hex_u32)]
        offset: u32,
        #[arg(long, value_enum, default_value_t = ModeArg::Lzs)]
        mode: ModeArg,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Scan every PROT entry directory, treating each file as a player.lzs
    /// container and reporting any that fully decode.
    Scan {
        dir: PathBuf,
        #[arg(long, default_value_t = 3)]
        count: usize,
    },
    /// Parse a buffer as a DATA_FIELD-style streaming container and dump
    /// each chunk's header + magic.
    Stream {
        input: PathBuf,
        #[arg(long, default_value_t = 4096)]
        max_chunks: usize,
    },
    /// Scan PROT entries for the streaming format used by FUN_8002541c 0x14.
    /// Reports entries that parse cleanly (terminator + all known types +
    /// all known magics match).
    ScanStream {
        dir: PathBuf,
        #[arg(long, default_value_t = 4096)]
        max_chunks: usize,
        /// Print only entries that fully validate.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Minimum chunk count to consider an entry "interesting".
        #[arg(long, default_value_t = 2)]
        min_chunks: usize,
    },
    /// Extract sub-assets from a streaming-format file. Each TIM_LIST and
    /// TMD chunk is unpacked using the [count, word_offsets, data] format.
    /// Each sub-asset is written to `<out>/chunk{i}_{TYPE}/{j}.{ext}`.
    Extract {
        input: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Also dump trailing data past the streaming terminator (if any)
        /// to `<out>/_trailer.bin` for later analysis.
        #[arg(long, default_value_t = true)]
        save_trailer: bool,
    },
    /// Bulk format classifier. Walks every file in `dir`, runs each known
    /// parser, and falls back to entropy/signature features. Emits a JSON
    /// report (default `<dir>/categorize.json`) plus a per-class summary.
    Categorize {
        dir: PathBuf,
        /// JSON output path. Defaults to `<dir>/categorize.json`.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Print top-N first-u32 signatures across the whole directory.
        #[arg(long, default_value_t = 20)]
        top_signatures: usize,
        /// Print up to N example file names per class.
        #[arg(long, default_value_t = 5)]
        examples: usize,
        /// Only list entries matching this class name (e.g. `unknown_other`,
        /// `unknown_high_entropy`). The JSON output is also filtered. Useful
        /// for sweeping Unknown* clusters without wading through the full
        /// report. Names are the `snake_case` strings from `Class::name()`.
        #[arg(long)]
        filter_class: Option<String>,
        /// CDNAME.TXT path. When given, the per-file listing and a slot-
        /// histogram table are cross-referenced with CDNAME scene blocks.
        /// For each matching entry the output shows `<scene>+<slot>` where
        /// `slot` is the zero-based offset within the scene block. Entries
        /// that don't fall inside any named block show `<raw_idx>` instead.
        /// Used to cross-reference unknown clusters against scene names.
        #[arg(long)]
        cdname: Option<PathBuf>,
    },
    /// Hunt for the 0x801C0000-overlay file. For each PROT entry, scan the
    /// raw bytes AND the result of LZS-decoding (at several plausible output
    /// sizes) for MIPS code-likelihood (`jr $ra` density, `addiu sp,sp,-N`
    /// prologue density, byte entropy in the code range). Reports the
    /// top-N candidates ranked by score.
    FindOverlay {
        dir: PathBuf,
        /// Number of top candidates to print.
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// LZS output sizes to try, comma-separated. Default covers the
        /// plausible overlay code range (32 KB .. 256 KB).
        #[arg(long, default_value = "32768,65536,98304,131072,196608,262144")]
        lzs_sizes: String,
    },
    /// Parse a per-summon stager overlay (extraction PROT 0903..=0913 player,
    /// 0914..=0923 evolved-Seru, 0927..=0934 high-summon, or the enemy boss
    /// block 0938/0940/0944/0961/0962/0966) into its move-VM part-record
    /// scene-graph: scan the `FUN_80021B04` + `FUN_80050ED4` spawn calls and
    /// print each part's record offset, mesh selector, flags, and bytecode
    /// span. Input is the raw overlay `.BIN`. Stager extraction files over-read
    /// into the following TOC entries - pass `--trim` with the entry's
    /// unique-content length (`(next_start_lba - start_lba) * 0x800`, accepts
    /// `0x` hex) to drop the neighbour bytes.
    SummonOverlay {
        input: PathBuf,
        /// Overlay link/load base (`*DAT_80010390`); default is the pinned
        /// shared summon-overlay buffer base.
        #[arg(long, value_parser = parse_hex_u32, default_value = "0x801F69D8")]
        base: u32,
        /// Trim the input to this many bytes before parsing (the entry's
        /// TOC-gap unique-content footprint). Value is **hex** - the `0x`
        /// prefix is optional, so `0x1800` and `1800` both mean 6144 bytes
        /// (a bare `6144` would be read as `0x6144`).
        #[arg(long, value_parser = parse_hex_u32)]
        trim: Option<u32>,
    },
    /// Parse a battle side-band streaming file - `summon.dat` (extraction PROT
    /// 0893) or `readef.DAT` (0894) - into its `0x10800`-byte slots and print
    /// each slot's class (texture / actor record / payload), texture layout,
    /// and attack-name string. `--texture-png-dir` additionally decodes every
    /// texture slot's 4bpp page through its first CLUT row to
    /// `slot_NNN.png`; `--action-id` prints which file + slot an action id
    /// streams from (the `FUN_801E295C` case-`0x32` banding).
    SummonReadef {
        /// Raw PROT 0893/0894 entry `.BIN`.
        input: PathBuf,
        /// Export texture slots as PNGs into this directory.
        #[arg(long)]
        texture_png_dir: Option<PathBuf>,
        /// CLUT sub-palette (16-color window) for 4bpp decode, 0..=15.
        #[arg(long, default_value_t = 0)]
        clut_sub: u8,
        /// Resolve an action id to its stream target and exit.
        #[arg(long)]
        action_id: Option<u8>,
    },
    /// Parse the battle-action per-move power table (runtime VA `0x801F4F5C`,
    /// read by `FUN_801dd0ac` for the arts/physical attacker roll) out of the
    /// raw PROT 0898 (battle-action overlay) `.BIN`. Prints each populated
    /// record's move id, decoded power (`+0` field `>> 2`), and raw bytes.
    MovePower {
        /// Raw PROT 0898 (battle-action overlay) entry `.BIN`.
        input: PathBuf,
        /// Emit the table as JSON instead of the text listing.
        #[arg(long)]
        json: bool,
    },
    /// Parse the 28-entry game-mode dispatch table (runtime VA `0x8007078C`)
    /// out of `SCUS_942.54`. Prints each mode's dev name, handler function
    /// pointer, parameter, and whether it routes through the shared per-frame
    /// handler. Recovers the index → retail-handler map from the disc.
    ModeTable {
        /// `SCUS_942.54` executable image.
        input: PathBuf,
        /// Emit the table as JSON instead of the text listing.
        #[arg(long)]
        json: bool,
    },
    /// Parse the battle element-affinity matrix (runtime VA `0x801F53E8`, read
    /// by `FUN_801dd864`) and the per-character element table (`0x801F5480`) out
    /// of the raw PROT 0898 (battle-action overlay) `.BIN`. Prints the 8×8
    /// matrix (`pct = matrix[attacker][defender]`) + each character's element.
    ElementAffinity {
        /// Raw PROT 0898 (battle-action overlay) entry `.BIN`.
        input: PathBuf,
        /// Emit the matrix + tables as JSON instead of the text listing.
        #[arg(long)]
        json: bool,
    },
    /// Print the player Seru-magic summon → namesake `battle_data` creature map
    /// (`legaia_asset::summon_creatures`, recovered from the disc by mesh
    /// identity). A static table, so no disc input is required; pass `--scus` to
    /// annotate each row with the summon's spell name.
    SummonCreatures {
        /// Optional `SCUS_942.54` image - adds each summon's spell name.
        #[arg(long)]
        scus: Option<PathBuf>,
        /// Emit the map as JSON instead of the text listing.
        #[arg(long)]
        json: bool,
    },
    /// Scan PROT entries (raw + LZS-decoded) for embedded PSX TIMs.
    /// Reports per-entry hit counts; with `--out` extracts each TIM to
    /// `<out>/<entry>/raw_off<H>.tim` (or `lzs<i>_off<H>.tim`).
    TimScan {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT for nicer names. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one hit.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Extract every found TIM into this directory.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Flat-scan a whole PROT.DAT image for standard PSX TIMs (jPSXdec
    /// parity) and emit a definitive per-TIM catalog keyed by a stable id
    /// (= jPSXdec's `PROT.DAT[<id>]` item index). Each row maps the TIM to
    /// its owning PROT entry + byte offset, with dimensions, CLUT count,
    /// byte length, and an FNV fingerprint. Unlike `tim-scan` (per-entry,
    /// lenient) this catches TIMs in the unindexed system-UI gap and applies
    /// strict validation so the result is the clean, jPSXdec-equivalent set.
    TimCatalog {
        /// Flat PROT.DAT image (e.g. `extracted/PROT.DAT`).
        prot: PathBuf,
        /// Write the catalog as JSON to this path. Default prints a table.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also print the count + rollup digest (the value the disc-gated
        /// regression pins).
        #[arg(long, default_value_t = false)]
        rollup: bool,
    },
    /// Deep TIM catalog: LZS-decompress every PROT entry and catalog the
    /// standard PSX TIMs found inside the decoded sections - the compressed
    /// character / scene textures the flat `tim-catalog` (raw-bytes only)
    /// can't see. Each row is keyed by `(entry, lzs-section, offset-in-
    /// section)` with dimensions, CLUT count, byte length, and an FNV
    /// fingerprint of the DECODED bytes. A hit is admitted only when the
    /// decoded bytes strict-parse AND decode to RGBA (LZS "decodes without
    /// error" is never a validity signal).
    TimDeepCatalog {
        /// PROT.DAT image (e.g. `extracted/PROT.DAT`).
        prot: PathBuf,
        /// Write the catalog as TSV (`.tsv`) or JSON to this path. Default
        /// prints a table.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also print the count + rollup digest (the value the disc-gated
        /// regression pins).
        #[arg(long, default_value_t = false)]
        rollup: bool,
    },
    /// Decode each DISTINCT cataloged TIM (raw + deep, deduped by content
    /// fingerprint) to a PNG plus a manifest, for local inspection /
    /// categorization. Output is decoded pixel data - keep it local, never
    /// commit it. Used to drive the `tim_labels` visual-categorization pass.
    TimRenderDistinct {
        /// PROT.DAT image (e.g. `extracted/PROT.DAT`).
        prot: PathBuf,
        /// Output directory; receives `<fnv>.png` per distinct texture and a
        /// `manifest.tsv`.
        #[arg(long)]
        out: PathBuf,
        /// Which tier(s) to render.
        #[arg(long, value_enum, default_value_t = RenderTier::Both)]
        tier: RenderTier,
    },
    /// Scan PROT entries (raw + LZS-decoded) for embedded Legaia TMDs.
    /// Reports per-entry hit counts and total verts/prims; with `--out`
    /// extracts each found TMD to `<out>/<entry>/raw_off<H>.tmd` (or
    /// `lzs<i>_off<H>.tmd` for LZS-section hits).
    TmdScan {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT for nicer names. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one hit.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
        /// Extract every found TMD into this directory.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Walk `tim_scan/<entry>/*.tim` under `extracted/` and report every
    /// TIM that places its CLUT or image at the requested VRAM cell. Used
    /// to discover which PROT entry provides a missing CLUT row that a
    /// character mesh references - the runtime asset chain is partially
    /// undocumented (see `project_clut_scattering.md`), and this is the
    /// principled discovery step before adding the TIM dir to the viewer's
    /// `--vram-extra-dir` set.
    ClutFinder {
        /// `extracted/` root (must contain `tim_scan/`).
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
        /// VRAM X coordinate (in 16-bit framebuffer units, 0..1024).
        x: u16,
        /// VRAM Y coordinate (0..512).
        y: u16,
        /// When set, only report TIMs whose CLUT covers the cell. Default
        /// reports BOTH CLUT and image cell hits, since a character mesh
        /// might reference either.
        #[arg(long, default_value_t = false)]
        clut_only: bool,
    },
    /// Inspect a stage-geometry PROT entry: detect the records table,
    /// pick the vertex pool side, print the first/last few records resolved
    /// to vertex indices, and a sample of the vertex pool.
    Stage {
        input: PathBuf,
        /// Number of records to print from the head of the table.
        #[arg(long, default_value_t = 8)]
        head: usize,
        /// Number of vertices to print from the head of the pool.
        #[arg(long, default_value_t = 8)]
        verts: usize,
        /// Optional output: write a wavefront-style OBJ of the wireframe
        /// quads (each record becomes 4 line segments - `l` directives).
        #[arg(short, long)]
        obj_out: Option<PathBuf>,
    },
    /// Bulk-scan a directory of PROT entries for stage-geometry tables.
    /// Reports per-entry records / pool size / how many records resolve to
    /// in-range vertex indices.
    StageScan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries with at least one record table.
        #[arg(long, default_value_t = true)]
        only_hits: bool,
    },
    /// Inspect a single PROT entry for the field-pack container shape
    /// (97-entry schema after `0x01059B84` magic). Reports preamble size,
    /// schema slot summary, and bytes-after-table.
    FieldPack {
        input: PathBuf,
        /// Print all 97 slot offsets/sizes (otherwise only first/last 8).
        #[arg(long, default_value_t = false)]
        all_slots: bool,
        /// Group slots by size and print the buckets in size-descending
        /// order. Slots in the same bucket are the same kind of record -
        /// the schema is byte-identical across every field-pack instance,
        /// so the cluster output is a static index of slot semantics.
        #[arg(long, default_value_t = false)]
        groups: bool,
    },
    /// Bulk-scan a directory of PROT entries for the field-pack format.
    /// Reports per-entry preamble size, table offset, and bytes-after-table.
    FieldPackScan {
        dir: PathBuf,
        /// Print only entries that match.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
    /// Inspect a single PROT entry for the effect-bundle container shape
    /// (28-entry schema after `0x02018B0C` magic). Reports preamble size,
    /// constant header words, and the schema slot summary.
    EffectBundle {
        input: PathBuf,
        /// Print all 28 slot offsets/sizes (otherwise only first/last 8).
        #[arg(long, default_value_t = false)]
        all_slots: bool,
    },
    /// Bulk-scan a directory of PROT entries for the effect-bundle format.
    /// Reports per-entry preamble size, table offset, and bytes-after-table.
    EffectBundleScan {
        dir: PathBuf,
        /// Print only entries that match.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
    /// Inspect a single PROT entry as a battle_data pack: list the record
    /// table, LZS-decode each record, and report which records hold a
    /// Legaia TMD. With `--out`, dumps the decompressed payload of every
    /// record to disk for downstream inspection.
    BattleDataPack {
        input: PathBuf,
        /// Optional output directory; written as `rec<NN>_id<HH>.bin` per
        /// record (decoded bytes; TMD at offset 0x20 when present).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Bulk-scan a directory of PROT entries for the battle_data pack format.
    /// Reports per-entry record count, decoded-payload total, and the number
    /// of records that hold a recognizable Legaia TMD.
    BattleDataPackScan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries that match.
        #[arg(long, default_value_t = true)]
        only_hits: bool,
    },
    /// Decode the monster stat archive (PROT entry `0867_battle_data`, the
    /// EXTENDED footprint). Prints one row per populated monster id with its
    /// name + HP/MP/stats. Pass `--id N` for a single monster.
    MonsterArchive {
        /// PROT entry 867 bytes (use the extended-footprint extract, e.g.
        /// `extracted/PROT/0867_battle_data.BIN`).
        input: PathBuf,
        /// Decode only this monster id (1-based).
        #[arg(long)]
        id: Option<u16>,
        /// Export the monster's embedded 3D mesh (the TMD at record +0x04) as
        /// a Wavefront OBJ to this path. Requires `--id`.
        #[arg(long)]
        obj: Option<PathBuf>,
        /// Export the monster's decoded texture page (the pool at record +0x08)
        /// as a PNG to this path. Requires `--id`.
        #[arg(long)]
        texture_png: Option<PathBuf>,
        /// Palette index (`cba & 0x3F`) to bake the texture PNG with. Defaults
        /// to the first textured prim's palette.
        #[arg(long)]
        palette: Option<usize>,
        /// List the monster's decoded action animations (part/frame counts and
        /// the idle action's per-object motion ranges). Requires `--id`.
        #[arg(long)]
        anim: bool,
        /// Export the monster's mesh + texture + all action animations as a
        /// binary glTF (`.glb`) to this path - a universal format that carries
        /// geometry, material, and animation together. Requires `--id`.
        #[arg(long)]
        glb: Option<PathBuf>,
    },
    /// Decode the player-character mesh pack at PROT entry `0874_befect_data`
    /// (§0). Prints the 5-slot shape (Vahn / Noa / Gala / + 2 auxiliary slots)
    /// with disc-form `nobj` and TMD body sizes; optionally applies the
    /// FUN_8001EBEC equipment-swap patch and writes the resulting TMD bytes.
    CharacterPack {
        /// PROT entry 874 bytes (extended footprint).
        input: PathBuf,
        /// Slot 0..=4 to inspect / patch (omit to print all).
        #[arg(long)]
        slot: Option<usize>,
        /// Equipment toggle byte (0 → group 11 template, anything else → group
        /// 10 template). Mirrors the per-character byte at record offsets
        /// 0x196 / 0x199 / 0x19B. Requires `--slot` and only applies to the
        /// 3 active-party slots (0..=2).
        #[arg(long)]
        equip: Option<u8>,
        /// Write the patched (or raw, if `--equip` is omitted) TMD body for
        /// `--slot` to this path. Format = disc-form Legaia TMD; parses with
        /// `legaia_tmd::parse`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Decode the battle-form character mesh pack at PROT entry
    /// `1204_other5`: five `TMD2` (asset type `0x09`) streaming chunks plus
    /// seven 256x256 4bpp character TIM atlases at fixed `0x8224` stride.
    /// This is the party's in-battle mesh set (Vahn / Noa / Gala + 2 extra
    /// fighters); the Baka Fighter fist-fight minigame reuses the same pack.
    /// The field-form pack (`character-pack`, PROT 0874 §0) is field-only.
    BattleCharPack {
        /// PROT entry 1204 bytes.
        input: PathBuf,
        /// Slot 0..=4 to inspect (omit to print all).
        #[arg(long)]
        slot: Option<usize>,
        /// Write the raw TMD body for `--slot` to this path (only with `--slot`).
        #[arg(long)]
        out_tmd: Option<PathBuf>,
        /// Atlas 0..=6 to write to `--out-tim` (only with `--out-tim`).
        #[arg(long)]
        atlas: Option<usize>,
        /// Write the raw TIM bytes of `--atlas` to this path.
        #[arg(long)]
        out_tim: Option<PathBuf>,
    },
    /// Decode the field-character texture pack at PROT 0874 **section 2**
    /// (the third LZS descriptor of `player.lzs`). Lists the eight TIM
    /// entries with their VRAM image / CLUT rects. Entries 1/2/3 are the
    /// Vahn/Noa/Gala field atlas pages (texpage `(832,256)`, CLUT row 478).
    /// With `--out-tim <PATH>` + `--entry <N>`, writes that entry's raw TIM.
    FieldCharTex {
        /// PROT entry 874 bytes (`extracted/PROT/0874_befect_data.BIN`).
        input: PathBuf,
        /// Entry 0..=7 to write to `--out-tim` (only with `--out-tim`).
        #[arg(long)]
        entry: Option<usize>,
        /// Write the raw TIM bytes of `--entry` to this path.
        #[arg(long)]
        out_tim: Option<PathBuf>,
    },
    /// Find every player-character animation bundle inside one PROT entry.
    /// Decodes the entry as a `parse_player_lzs`-shaped container, walks each
    /// type-0x05 ("MOVE") section, LZS-decompresses it, and reports
    /// containers that parse as canonical ANM data (records starting with
    /// `marker_1 = 0x080C`). With `--out`, writes each bundle's decoded
    /// bytes to `<out>/<entry>_sect<i>.anm`.
    PlayerAnm {
        /// PROT entry to inspect (e.g. `extracted/PROT/0004_town01.BIN`).
        input: PathBuf,
        /// `parse_player_lzs` descriptor count (typically 3, 5, 6, or 7).
        #[arg(long, default_value_t = 6)]
        desc_count: usize,
        /// Write each cleanly-parsed bundle's LZS-decoded bytes here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Bulk-scan every PROT entry under `dir` for player-ANM bundles. Reports
    /// per-entry record count + decoded-bytes for each type-0x05 section that
    /// parses cleanly. With `--cdname`, prefixes each line with the CDNAME
    /// scene label.
    PlayerAnmScan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// `parse_player_lzs` descriptor count.
        #[arg(long, default_value_t = 6)]
        desc_count: usize,
    },
    /// Inspect a single PROT entry as a scene v12 table: print the header
    /// fields, the inline records at `+0x14`, and a summary of the
    /// event-script prescript at `+0x800`.
    SceneV12 {
        input: PathBuf,
        /// Print every event-script record's bytecode head (first 16 bytes)
        /// instead of just the count and frame-opener rate.
        #[arg(long, default_value_t = false)]
        scripts: bool,
        /// Maximum script records to print when `--scripts` is on.
        #[arg(long, default_value_t = 16)]
        max_scripts: usize,
    },
    /// Bulk-scan a directory of PROT entries for the scene_v12_table format.
    /// Reports per-entry `N` / `param` / inline-records group counts /
    /// script-record count / frame-opener rate. With `--cdname`, prefixes
    /// each line with the CDNAME scene label.
    SceneV12Scan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Print only entries that match.
        #[arg(long, default_value_t = true)]
        only_hits: bool,
    },
    /// Parse the world-map quick-travel menu out of a `SCUS_942.54`
    /// executable: the 16-entry landmark name table at `DAT_80073B18`
    /// plus the 6-byte placement records at `DAT_80073A98`. Prints either
    /// a human-readable table or the same JSON shape the web viewer
    /// consumes (with `--json`).
    WorldmapMenu {
        /// Path to `SCUS_942.54` (typically `extracted/SCUS_942.54`).
        scus: PathBuf,
        /// Emit machine-readable JSON instead of the formatted table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the static item tables out of a `SCUS_942.54`: per item id the
    /// name + the consumable effect descriptor (`DAT_800752C0`) or the
    /// equipment stat-bonus record (`DAT_80074F68`). See
    /// `docs/formats/item-effect-table.md` and `equipment-table.md`.
    ItemTables {
        /// Path to `SCUS_942.54` (typically `extracted/SCUS_942.54`).
        scus: PathBuf,
        /// Only print equippable items.
        #[arg(long, default_value_t = false)]
        equipment_only: bool,
        /// Only print usable consumables.
        #[arg(long, default_value_t = false)]
        consumables_only: bool,
    },
    /// Dump the static spell name / MP / target table from `SCUS_942.54`
    /// (`legaia_asset::spell_names`, `DAT_800754C8`). See
    /// `docs/formats/spell-table.md`.
    SpellNames {
        /// Path to `SCUS_942.54` (typically `extracted/SCUS_942.54`).
        scus: PathBuf,
        /// Emit JSON (an array of `{id, name, mp, target}`) instead of text.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the static per-monster steal table from `SCUS_942.54`
    /// (`legaia_asset::steal_table`, `DAT_80077828`) - what the Evil God
    /// Icon steals - joining each stolen item id to its name. See
    /// `docs/formats/steal-table.md`.
    StealTable {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
        /// Print every monster id, including non-stealable rows.
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Emit JSON (an array of `{monster_id, chance_pct, item_id, item_name}`).
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the 64-slot accessory ("Goods") passive-effect table from
    /// `SCUS_942.54` (`legaia_asset::accessory_passive`, `0x8007625C`). See
    /// `docs/formats/accessory-passive-table.md`.
    AccessoryPassive {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
        /// Emit JSON (an array of `{index, name, party_wide, boosts}`).
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the sound-effect descriptor table from `SCUS_942.54`
    /// (`legaia_asset::sfx_table`, `DAT_8006F198`, 100 cues). See
    /// `docs/formats/sfx-table.md`.
    SfxTable {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
        /// Emit JSON (an array of `{id, ...descriptor}`) instead of text.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the new-game starting-party template + starting inventory from
    /// `SCUS_942.54` (`legaia_asset::new_game`, `0x80078C4C`). See
    /// `docs/formats/new-game-table.md`.
    NewGame {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
        /// Emit JSON (`{party: [...], inventory: [...]}`) instead of text.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Dump the per-character stat-growth params + XP thresholds from
    /// `SCUS_942.54` (`legaia_asset::level_up_tables`, `DAT_80076918`). See
    /// `docs/reference/gamedata.md` and the stat-growth thread.
    LevelUp {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
        /// Emit JSON (`{growth: [...], xp_thresholds: [...]}`) instead of text.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Render a kingdom's slot-4 wireframe (or a raw decoded slot-4 .bin)
    /// to a top-down PNG. The output uses the same per-body color palette
    /// as the WebGL world-overview viewer so a PNG screenshot can be
    /// visually diffed against the in-browser render.
    ///
    /// Two input modes:
    ///   - `--input <PROT>.BIN` : a kingdom PROT entry; LZS-decodes slot 4.
    ///   - `--from-raw <slot4>.bin`: a previously-decoded slot-4 payload
    ///     (e.g. dumped from live RAM via the PCSX-Redux autorun).
    ///
    /// Optional `--placements <world-overview.json>` overlays the kingdom's
    /// MAN-asset placements as dots so you can verify landmarks sit inside
    /// the (falsified) coastline-curve reading.
    Slot4Png {
        /// Kingdom PROT entry to decode. Mutually exclusive with `--from-raw`.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Already-decoded slot-4 .bin (e.g. RAM dump). Mutually exclusive
        /// with `--input`.
        #[arg(long)]
        from_raw: Option<PathBuf>,
        /// Output PNG path.
        #[arg(short, long)]
        out: PathBuf,
        /// `site/world-overview.json` (or world-overview-live.json) used
        /// for an optional placement-scatter overlay. The kingdom key is
        /// derived from `--kingdom`.
        #[arg(long)]
        placements: Option<PathBuf>,
        /// Kingdom key for the placement overlay (`drake` / `sebucus` /
        /// `karisto`). Ignored when `--placements` is absent.
        #[arg(long, default_value = "drake")]
        kingdom: String,
        /// PNG width in pixels.
        #[arg(long, default_value_t = 1024)]
        width: u32,
        /// PNG height in pixels.
        #[arg(long, default_value_t = 1024)]
        height: u32,
        /// Pixel margin around the world bbox.
        #[arg(long, default_value_t = 16)]
        margin: u32,
        /// Render only this body index (0..N-1). Useful for isolating
        /// body 12 in Drake from the noisy
        /// inner contours.
        #[arg(long)]
        only_body: Option<usize>,
        /// Frame the camera on a single body's bbox instead of the full
        /// slot-4 extent. When set, body 13's full-extent (kind-4) records are
        /// skipped from the camera fit, so the inner contours fill
        /// the canvas instead of compressing into a corner.
        #[arg(long)]
        frame_body: Option<usize>,
        /// Close each polyline back to its first vertex. Off by default
        /// (slot-4 groups are open polylines, not closed polygons).
        #[arg(long, default_value_t = false)]
        close_polylines: bool,
        /// Polyline-construction mode. `row` connects each group's
        /// records in order (matches the WebGL world-overview viewer).
        /// `col` connects each record-slot's value across groups.
        /// `pairs` emits each consecutive pair of records as one line
        /// segment (so `count_a = 10` becomes 5 segments per group);
        /// useful for the slot-4 contour-pair hypothesis.
        /// `grid` draws the body as a `count_a` x `count_b` heightfield
        /// quad mesh (both row and column edges) - the most likely
        /// topology for slot-4 body 12.
        /// `points` is topology-free - one dot per record, no line
        /// segments at all. Use `points` for raw-data validation.
        #[arg(long, default_value = "row")]
        style: String,
        /// Output 2D axes as a two-character string from `x|y|z`.
        /// Default `xz` (top-down map). Try `xy` or `zy` to see whether
        /// a body has 3D side-view structure that XZ flattens away -
        /// Drake bodies 9 / 11 / 12 reveal coherent silhouettes only in
        /// the non-default projections.
        #[arg(long, default_value = "xz")]
        axes: String,
    },
    /// Decode one slot of a kingdom-bundle PROT entry (map01 / map02 / map03).
    ///
    /// Locates the 7-asset table, LZS-decodes the requested slot, and prints
    /// a structural summary. When `slot=4` (world-map overlay outlines), also
    /// dumps the per-body inventory. Optional `--out` writes the raw decoded
    /// bytes; optional `--wireframe-obj` writes the slot-4 wireframe as a
    /// Wavefront OBJ for inspection in any 3D viewer.
    KingdomSlot {
        /// Path to a kingdom PROT entry buffer (e.g. `extracted/PROT/0085_xxx.BIN`).
        input: PathBuf,
        /// Slot index (0..7). Slot 4 = world-map overlay outlines.
        #[arg(long, default_value_t = 4)]
        slot: u8,
        /// Write the raw decoded bytes to this path.
        #[arg(long)]
        out: Option<PathBuf>,
        /// When `slot=4`: write a Wavefront OBJ of the wireframe (lines only).
        #[arg(long)]
        wireframe_obj: Option<PathBuf>,
    },
    /// Inspect one PROT entry's MAN (asset type 0x03) sub-asset:
    /// LZS-decode the third descriptor of a `scene_asset_table` bundle
    /// and walk the multi-section header at FUN_8003AEB0. Prints the
    /// header fields, every section's offset+length, and (when `--with-encounter`)
    /// decodes section 0 as the encounter section (FUN_8003A110).
    Man {
        /// PROT entry (`extracted/PROT/0086_map01.BIN`).
        input: PathBuf,
        /// Also decode and print the section-0 (encounter) interior.
        #[arg(long, default_value_t = false)]
        with_encounter: bool,
        /// Limit how many formation records to print when `--with-encounter`.
        #[arg(long, default_value_t = 16)]
        max_formations: usize,
        /// Limit how many region records to print when `--with-encounter`.
        #[arg(long, default_value_t = 16)]
        max_regions: usize,
    },
    /// Bulk-scan a directory of PROT entries for the MAN multi-section
    /// shape (asset 0x03 inside `scene_asset_table` bundles). Reports
    /// per-scene the partition counts, encounter-section offset, and
    /// section 1..4 lengths.
    ManScan {
        dir: PathBuf,
        /// CDNAME.TXT for nicer entry titles. Optional.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Emit JSON instead of a formatted table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Targeted validation: walk PROT entries that correspond to the first
    /// entry of each named CDNAME block. Each is tested with strict layout
    /// and (when applicable) magic checks.
    Validate {
        /// Directory of extracted PROT entries (e.g. `extracted/PROT`).
        dir: PathBuf,
        /// CDNAME.TXT path (block boundaries). If absent, scan ALL entries.
        #[arg(long)]
        cdname: Option<PathBuf>,
        /// Try this many descriptor counts; report the best.
        #[arg(long, default_value = "1,2,3,4,8,16,32")]
        counts: String,
        /// Print only blocks whose first entry validates.
        #[arg(long, default_value_t = false)]
        only_hits: bool,
    },
    /// Cluster-aware extraction of the battle-effect `befect_data` cluster.
    /// The per-entry PROT extractor over-reads here (the entries overlap on
    /// disc), so this slices each entry at its true footprint, expands the
    /// LZS-container entry into its sub-files, classifies every part
    /// (`efect.dat` 2-pack / effect-model TMDs / effect-texture TIMs / packs),
    /// and optionally writes the clean parts to `--out`.
    BefectCluster {
        /// Path to `PROT.DAT`.
        prot: PathBuf,
        /// Path to `CDNAME.TXT` (locates the `befect_data` cluster).
        #[arg(long)]
        cdname: PathBuf,
        /// Write each classified part to this directory.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Emit a JSON manifest instead of a formatted table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Static overlay-extraction pipeline: extract each clean-copy runtime
    /// overlay from PROT.DAT in its as-loaded form, with identity attached from
    /// the source entry. Complements (does not replace) the dynamic save-state
    /// captures. See docs/tooling/static-overlay-pipeline.md.
    Overlay {
        #[command(subcommand)]
        cmd: OverlayCmd,
    },
}

#[derive(Subcommand)]
enum OverlayCmd {
    /// Print the committed static-overlay map (PROT index -> base -> identity).
    List {
        /// Emit JSON instead of a table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Extract each eligible overlay's as-loaded bytes to a gitignored .bin in
    /// `--out` (these are Sony code; the dir is expected to be gitignored).
    Extract {
        /// Path to `PROT.DAT`.
        prot_dat: PathBuf,
        /// Output directory for the `.bin` blobs.
        #[arg(long)]
        out: PathBuf,
        /// Only this overlay label (default: every eligible overlay).
        #[arg(long)]
        label: Option<String>,
    },
    /// Re-extract from PROT.DAT and assert each overlay's as-loaded bytes hash
    /// to the committed fingerprint (disc-gated reproducibility check).
    Verify {
        /// Path to `PROT.DAT`.
        prot_dat: PathBuf,
    },
    /// Emit Ghidra import helpers: a per-overlay Jython rename script and a
    /// shell driver that imports each overlay at its base into the compose
    /// service (program named `overlay_<label>`).
    Ghidra {
        /// Output directory for the generated scripts.
        #[arg(long)]
        out: PathBuf,
    },
    /// Sweep a range of PROT entries: recover each base statically, count the
    /// votes, and print the leading dev string (the identity tell). Use this to
    /// enumerate the overlay corpus and triage which entries carry pinned
    /// identity - a reproducible reconnaissance view, not committed anywhere.
    Scan {
        /// Path to `PROT.DAT`.
        prot_dat: PathBuf,
        /// First PROT index to scan (inclusive).
        #[arg(long, default_value_t = 895)]
        from: u32,
        /// Last PROT index to scan (inclusive).
        #[arg(long, default_value_t = 985)]
        to: u32,
        /// Minimum corroborating call targets to report a recovered base.
        #[arg(long, default_value_t = 8)]
        min_votes: u32,
        /// Only print entries whose base recovers to this VA (e.g. the slot-A
        /// base 0x801CE818) - filters the sweep to one overlay slot.
        #[arg(long, value_parser = parse_hex_u32)]
        base: Option<u32>,
        /// Emit JSON instead of a table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Locate a function-head instruction signature across the corpus and, given
    /// the function's known VA, infer the host overlay's load base
    /// (`base = anchor_va - file_offset`). This is the byte-search that pins an
    /// overlay's PROT entry with no capture - how the menu overlay (0899) was
    /// found from `FUN_801CF650`'s signature.
    FindSig {
        /// Path to `PROT.DAT`.
        prot_dat: PathBuf,
        /// Function-head signature: little-endian machine code of the first few
        /// instructions, as hex (spaces allowed), e.g. "1e80043c a046838c".
        sig_hex: String,
        /// Known VA of the function whose head this is; prints the implied base.
        #[arg(long, value_parser = parse_hex_u32)]
        anchor_va: Option<u32>,
        /// First PROT index to search (inclusive).
        #[arg(long, default_value_t = 0)]
        from: u32,
        /// Last PROT index to search (inclusive).
        #[arg(long, default_value_t = 1234)]
        to: u32,
    },
    /// Regenerate map rows from PROT.DAT: recover each base statically and hash
    /// the as-loaded bytes. Prints TOML to stdout (review, then paste into
    /// `crates/asset/data/static-overlays.toml`).
    Generate {
        /// Path to `PROT.DAT`.
        prot_dat: PathBuf,
        /// PROT index to (re)derive a row for. Repeatable. Default: refresh
        /// every index already in the committed map.
        #[arg(long = "index")]
        indices: Vec<u32>,
        /// Minimum corroborating call targets to accept a recovered base.
        #[arg(long, default_value_t = 8)]
        min_votes: u32,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Lzs,
    Raw,
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Describe { input, count } => describe(&input, count),
        Cmd::Decode {
            input,
            type_size,
            offset,
            mode,
            out,
        } => decode_one(&input, type_size, offset, mode, out.as_ref()),
        Cmd::Scan { dir, count } => scan(&dir, count),
        Cmd::Stream { input, max_chunks } => stream_one(&input, max_chunks),
        Cmd::ScanStream {
            dir,
            max_chunks,
            only_hits,
            min_chunks,
        } => scan_stream(&dir, max_chunks, only_hits, min_chunks),
        Cmd::Extract {
            input,
            out,
            save_trailer,
        } => extract_streaming(&input, &out, save_trailer),
        Cmd::TimScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tim_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::TimCatalog { prot, out, rollup } => tim_catalog_cmd(&prot, out.as_deref(), rollup),
        Cmd::TimDeepCatalog { prot, out, rollup } => {
            tim_deep_catalog_cmd(&prot, out.as_deref(), rollup)
        }
        Cmd::TimRenderDistinct { prot, out, tier } => tim_render_distinct_cmd(&prot, &out, tier),
        Cmd::TmdScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tmd_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::ClutFinder {
            extracted_root,
            x,
            y,
            clut_only,
        } => clut_finder_cmd(&extracted_root, x, y, clut_only),
        Cmd::Stage {
            input,
            head,
            verts,
            obj_out,
        } => stage_one(&input, head, verts, obj_out.as_deref()),
        Cmd::StageScan {
            dir,
            cdname,
            only_hits,
        } => stage_scan_cmd(&dir, cdname.as_deref(), only_hits),
        Cmd::FieldPack {
            input,
            all_slots,
            groups,
        } => field_pack_one(&input, all_slots, groups),
        Cmd::FieldPackScan { dir, only_hits } => field_pack_scan(&dir, only_hits),
        Cmd::EffectBundle { input, all_slots } => effect_bundle_one(&input, all_slots),
        Cmd::BattleDataPack { input, out } => battle_data_pack_one(&input, out.as_deref()),
        Cmd::BattleDataPackScan {
            dir,
            cdname,
            only_hits,
        } => battle_data_pack_scan(&dir, cdname.as_deref(), only_hits),
        Cmd::EffectBundleScan { dir, only_hits } => effect_bundle_scan(&dir, only_hits),
        Cmd::PlayerAnm {
            input,
            desc_count,
            out,
        } => player_anm_one(&input, desc_count, out.as_deref()),
        Cmd::PlayerAnmScan {
            dir,
            cdname,
            desc_count,
        } => player_anm_scan(&dir, cdname.as_deref(), desc_count),
        Cmd::SceneV12 {
            input,
            scripts,
            max_scripts,
        } => scene_v12_one(&input, scripts, max_scripts),
        Cmd::SceneV12Scan {
            dir,
            cdname,
            only_hits,
        } => scene_v12_scan(&dir, cdname.as_deref(), only_hits),
        Cmd::WorldmapMenu { scus, json } => worldmap_menu_cmd(&scus, json),
        Cmd::ItemTables {
            scus,
            equipment_only,
            consumables_only,
        } => item_tables_cmd(&scus, equipment_only, consumables_only),
        Cmd::SpellNames { scus, json } => spell_names_cmd(&scus, json),
        Cmd::StealTable { scus, all, json } => steal_table_cmd(&scus, all, json),
        Cmd::AccessoryPassive { scus, json } => accessory_passive_cmd(&scus, json),
        Cmd::SfxTable { scus, json } => sfx_table_cmd(&scus, json),
        Cmd::NewGame { scus, json } => new_game_cmd(&scus, json),
        Cmd::LevelUp { scus, json } => level_up_cmd(&scus, json),
        Cmd::KingdomSlot {
            input,
            slot,
            out,
            wireframe_obj,
        } => kingdom_slot_cmd(&input, slot, out.as_deref(), wireframe_obj.as_deref()),
        Cmd::Slot4Png {
            input,
            from_raw,
            out,
            placements,
            kingdom,
            width,
            height,
            margin,
            only_body,
            frame_body,
            close_polylines,
            style,
            axes,
        } => slot4_png_cmd(
            input.as_deref(),
            from_raw.as_deref(),
            &out,
            placements.as_deref(),
            &kingdom,
            width,
            height,
            margin,
            only_body,
            frame_body,
            close_polylines,
            &style,
            &axes,
        ),
        Cmd::Man {
            input,
            with_encounter,
            max_formations,
            max_regions,
        } => man_one(&input, with_encounter, max_formations, max_regions),
        Cmd::ManScan { dir, cdname, json } => man_scan(&dir, cdname.as_deref(), json),
        Cmd::MonsterArchive {
            input,
            id,
            obj,
            texture_png,
            palette,
            anim,
            glb,
        } => monster_archive_one(
            &input,
            id,
            obj.as_deref(),
            texture_png.as_deref(),
            palette,
            anim,
            glb.as_deref(),
        ),
        Cmd::CharacterPack {
            input,
            slot,
            equip,
            out,
        } => character_pack_one(&input, slot, equip, out.as_deref()),
        Cmd::BattleCharPack {
            input,
            slot,
            out_tmd,
            atlas,
            out_tim,
        } => battle_char_pack_one(&input, slot, out_tmd.as_deref(), atlas, out_tim.as_deref()),
        Cmd::FieldCharTex {
            input,
            entry,
            out_tim,
        } => field_char_tex_one(&input, entry, out_tim.as_deref()),
        Cmd::Validate {
            dir,
            cdname,
            counts,
            only_hits,
        } => validate_blocks(&dir, cdname.as_ref(), &counts, only_hits),
        Cmd::BefectCluster {
            prot,
            cdname,
            out,
            json,
        } => befect_cluster_cmd(&prot, &cdname, out.as_deref(), json),
        Cmd::Categorize {
            dir,
            out,
            top_signatures,
            examples,
            filter_class,
            cdname,
        } => categorize_dir(
            &dir,
            out.as_ref(),
            top_signatures,
            examples,
            filter_class.as_deref(),
            cdname.as_deref(),
        ),
        Cmd::FindOverlay {
            dir,
            top,
            lzs_sizes,
        } => find_overlay(&dir, top, &lzs_sizes),
        Cmd::SummonOverlay { input, base, trim } => summon_overlay_cmd(&input, base, trim),
        Cmd::SummonReadef {
            input,
            texture_png_dir,
            clut_sub,
            action_id,
        } => summon_readef_cmd(&input, texture_png_dir.as_deref(), clut_sub, action_id),
        Cmd::MovePower { input, json } => move_power_cmd(&input, json),
        Cmd::ModeTable { input, json } => mode_table_cmd(&input, json),
        Cmd::ElementAffinity { input, json } => element_affinity_cmd(&input, json),
        Cmd::SummonCreatures { scus, json } => summon_creatures_cmd(scus.as_deref(), json),
        Cmd::Overlay { cmd } => match cmd {
            OverlayCmd::List { json } => overlay_list_cmd(json),
            OverlayCmd::Extract {
                prot_dat,
                out,
                label,
            } => overlay_extract_cmd(&prot_dat, &out, label.as_deref()),
            OverlayCmd::Verify { prot_dat } => overlay_verify_cmd(&prot_dat),
            OverlayCmd::Ghidra { out } => overlay_ghidra_cmd(&out),
            OverlayCmd::Generate {
                prot_dat,
                indices,
                min_votes,
            } => overlay_generate_cmd(&prot_dat, &indices, min_votes),
            OverlayCmd::Scan {
                prot_dat,
                from,
                to,
                min_votes,
                base,
                json,
            } => overlay_scan_cmd(&prot_dat, from, to, min_votes, base, json),
            OverlayCmd::FindSig {
                prot_dat,
                sig_hex,
                anchor_va,
                from,
                to,
            } => overlay_find_sig_cmd(&prot_dat, &sig_hex, anchor_va, from, to),
        },
    }
}

/// Which catalog tier(s) `tim-render-distinct` decodes.
#[derive(Copy, Clone, Debug, ValueEnum)]
enum RenderTier {
    /// Flat (raw-bytes) catalog only.
    Raw,
    /// LZS-embedded (deep) catalog only.
    Deep,
    /// Both tiers, deduped by fingerprint (raw takes the representative).
    Both,
}

fn parse_hex_u32(s: &str) -> std::result::Result<u32, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).map_err(|e| e.to_string())
}
