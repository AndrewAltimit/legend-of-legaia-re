use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use legaia_asset::{
    AssetType, DecodeMode, Descriptor, battle_data_pack, categorize, decode, effect_bundle,
    field_pack, parse_player_lzs, parse_streaming, stage_geom, tim_catalog, tim_deep_catalog,
    tim_scan, tmd_scan, validate,
};
use legaia_prot::cdname;

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
    /// into the following TOC entries — pass `--trim` with the entry's
    /// unique-content length (`(next_start_lba - start_lba) * 0x800`, accepts
    /// `0x` hex) to drop the neighbour bytes.
    SummonOverlay {
        input: PathBuf,
        /// Overlay link/load base (`*DAT_80010390`); default is the pinned
        /// shared summon-overlay buffer base.
        #[arg(long, value_parser = parse_hex_u32, default_value = "0x801F69D8")]
        base: u32,
        /// Trim the input to this many bytes before parsing (the entry's
        /// TOC-gap unique-content footprint). Value is **hex** — the `0x`
        /// prefix is optional, so `0x1800` and `1800` both mean 6144 bytes
        /// (a bare `6144` would be read as `0x6144`).
        #[arg(long, value_parser = parse_hex_u32)]
        trim: Option<u32>,
    },
    /// Parse a battle side-band streaming file — `summon.dat` (extraction PROT
    /// 0893) or `readef.DAT` (0894) — into its `0x10800`-byte slots and print
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
    },
    /// Parse the 28-entry game-mode dispatch table (runtime VA `0x8007078C`)
    /// out of `SCUS_942.54`. Prints each mode's dev name, handler function
    /// pointer, parameter, and whether it routes through the shared per-frame
    /// handler. Recovers the index → retail-handler map from the disc.
    ModeTable {
        /// `SCUS_942.54` executable image.
        input: PathBuf,
    },
    /// Parse the battle element-affinity matrix (runtime VA `0x801F53E8`, read
    /// by `FUN_801dd864`) and the per-character element table (`0x801F5480`) out
    /// of the raw PROT 0898 (battle-action overlay) `.BIN`. Prints the 8×8
    /// matrix (`pct = matrix[attacker][defender]`) + each character's element.
    ElementAffinity {
        /// Raw PROT 0898 (battle-action overlay) entry `.BIN`.
        input: PathBuf,
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
        /// binary glTF (`.glb`) to this path — a universal format that carries
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
    },
    /// Dump the 64-slot accessory ("Goods") passive-effect table from
    /// `SCUS_942.54` (`legaia_asset::accessory_passive`, `0x8007625C`). See
    /// `docs/formats/accessory-passive-table.md`.
    AccessoryPassive {
        /// Path to `SCUS_942.54`.
        scus: PathBuf,
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
    /// identity — a reproducible reconnaissance view, not committed anywhere.
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
        /// base 0x801CE818) — filters the sweep to one overlay slot.
        #[arg(long, value_parser = parse_hex_u32)]
        base: Option<u32>,
        /// Emit JSON instead of a table.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Locate a function-head instruction signature across the corpus and, given
    /// the function's known VA, infer the host overlay's load base
    /// (`base = anchor_va - file_offset`). This is the byte-search that pins an
    /// overlay's PROT entry with no capture — how the menu overlay (0899) was
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
        Cmd::SpellNames { scus } => spell_names_cmd(&scus),
        Cmd::StealTable { scus, all } => steal_table_cmd(&scus, all),
        Cmd::AccessoryPassive { scus } => accessory_passive_cmd(&scus),
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
        Cmd::MovePower { input } => move_power_cmd(&input),
        Cmd::ModeTable { input } => mode_table_cmd(&input),
        Cmd::ElementAffinity { input } => element_affinity_cmd(&input),
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

/// Parse the battle-action per-move power table and print its records.
fn mode_table_cmd(input: &Path) -> Result<()> {
    let bytes = std::fs::read(input)?;
    let Some(table) = legaia_asset::mode_table::ModeTable::from_scus(&bytes) else {
        anyhow::bail!(
            "no game-mode table at VA {:#x} in {} - is this SCUS_942.54?",
            legaia_asset::mode_table::MODE_TABLE_VA,
            input.display(),
        );
    };
    println!(
        "game-mode table @ VA {:#010x} ({} entries)",
        legaia_asset::mode_table::MODE_TABLE_VA,
        table.entries.len()
    );
    println!(
        "{:>3}  {:<14}  {:<10}  {:<10}  kind",
        "idx", "name", "handler", "param"
    );
    for e in &table.entries {
        let kind = if e.is_per_frame() {
            if e.uses_shared_handler() {
                "per-frame (shared)"
            } else {
                "per-frame"
            }
        } else {
            "init"
        };
        println!(
            "{:>3}  {:<14}  {:#010x}  {:#010x}  {kind}",
            e.index, e.name, e.handler, e.param
        );
    }
    println!(
        "{} of {} per-frame modes share handler {:#010x}",
        table.shared_handler_count(),
        table.entries.iter().filter(|e| e.is_per_frame()).count(),
        legaia_asset::mode_table::SHARED_PER_FRAME_HANDLER,
    );
    Ok(())
}

fn move_power_cmd(input: &Path) -> Result<()> {
    let bytes = std::fs::read(input)?;
    let Some(table) = legaia_asset::move_power::parse(&bytes) else {
        anyhow::bail!(
            "no move-power table at the pinned offset {:#x} in {} ({} bytes) - \
             is this the raw PROT 0898 battle-action overlay entry?",
            legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    let map = legaia_asset::move_power::parse_id_index_map(&bytes);
    // Invert the map: power index -> the move id(s) that resolve to it.
    let move_ids_for = |idx: usize| -> String {
        match &map {
            None => String::new(),
            Some(m) => {
                let ids: Vec<String> = (0..m.len())
                    .filter(|&mid| {
                        legaia_asset::move_power::index_for_move_id(m, mid as u8) == Some(idx as u8)
                    })
                    .map(|mid| format!("{mid:#04x}"))
                    .collect();
                if ids.is_empty() {
                    String::new()
                } else {
                    format!("  <- move {}", ids.join(","))
                }
            }
        }
    };
    println!(
        "move-power table: {} records @ file {:#x} (runtime VA {:#010x}), 26-byte stride; \
         id->index map {}",
        table.len(),
        legaia_asset::move_power::MOVE_POWER_TABLE_FILE_OFFSET,
        legaia_asset::move_power::MOVE_POWER_TABLE_VA,
        if map.is_some() { "present" } else { "MISSING" },
    );
    for r in &table {
        if r.is_empty() {
            continue;
        }
        let tag = r
            .annotation_tag()
            .map(|c| c.to_string())
            .unwrap_or_default();
        let contact = r.contact_effects();
        let launch = r.launch_effects();
        let fx = |v: &[u8]| -> String {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.iter()
                    .map(|b| format!("{b:#04x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        println!(
            "  idx {:3}  power {:5} (raw {:#06x})  ctr {:4}  phase {:4}  homing {:#04x}  \
             yoff {:5}  impact {}  trail {}  sfx {:#04x}  list {:#04x}  tag {:1}  \
             contact[{}]  launch[{}]{}",
            r.index,
            r.power(),
            r.power_raw as u16,
            r.counter_init(),
            r.phase_duration(),
            r.homing_speed(),
            r.strike_y_offset(),
            r.impact_effect(),
            r.trail_texture_page(),
            r.sound_cue_id(),
            r.list_mode(),
            tag,
            fx(&contact),
            fx(&launch),
            move_ids_for(r.index),
        );
    }
    Ok(())
}

/// Parse + print the battle element-affinity matrix and per-character table.
fn element_affinity_cmd(input: &Path) -> Result<()> {
    use legaia_asset::element_affinity::{self, Element};
    let bytes = std::fs::read(input)?;
    let Some(aff) = element_affinity::parse(&bytes) else {
        anyhow::bail!(
            "no element-affinity tables at the pinned offsets (matrix {:#x}, \
             char table {:#x}) in {} ({} bytes) - is this the raw PROT 0898 \
             battle-action overlay entry?",
            element_affinity::AFFINITY_MATRIX_FILE_OFFSET,
            element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET,
            input.display(),
            bytes.len(),
        );
    };
    let label = |id: usize| -> String {
        Element::from_id(id as u8)
            .map(|e| e.name().to_string())
            .unwrap_or_else(|| format!("?{id}"))
    };
    println!(
        "element-affinity matrix @ file {:#x} (runtime VA {:#010x}); pct = matrix[attacker][defender]",
        element_affinity::AFFINITY_MATRIX_FILE_OFFSET,
        element_affinity::AFFINITY_MATRIX_VA,
    );
    print!("atk\\def ");
    for def in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>8}", label(def));
    }
    println!();
    for atk in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>7} ", label(atk));
        for def in 0..element_affinity::ELEMENT_COUNT {
            print!("{:>8}", aff.matrix[atk][def]);
        }
        println!();
    }
    println!(
        "\nper-character element table @ file {:#x} (runtime VA {:#010x}, 1-based char id):",
        element_affinity::CHARACTER_ELEMENTS_FILE_OFFSET,
        element_affinity::CHARACTER_ELEMENTS_VA,
    );
    let names = ["Vahn", "Noa", "Gala", "Terra"];
    for (i, &elem) in aff.character_elements.iter().enumerate() {
        let who = names.get(i).copied().unwrap_or("");
        println!(
            "  char {:>2} {:<6} -> element {} ({})",
            i + 1,
            who,
            elem,
            label(elem as usize)
        );
    }
    println!(
        "\nper-character summon power-percent @ file {:#x} (runtime VA {:#010x}); \
         pct = row[summon creature element], FUN_801ddb30 stage 5:",
        legaia_asset::element_affinity::SUMMON_POWER_PCT_FILE_OFFSET,
        legaia_asset::element_affinity::SUMMON_POWER_PCT_VA,
    );
    print!("        ");
    for elem in 0..element_affinity::ELEMENT_COUNT {
        print!("{:>8}", label(elem));
    }
    println!();
    for (i, row) in aff.summon_power.iter().enumerate() {
        print!("{:>7} ", names.get(i).copied().unwrap_or(""));
        for pct in row {
            print!("{pct:>8}");
        }
        println!();
    }
    Ok(())
}

/// Parse a per-summon stager overlay and print its move-VM part-record list.
fn summon_overlay_cmd(input: &Path, base: u32, trim: Option<u32>) -> Result<()> {
    let mut bytes = std::fs::read(input)?;
    let full_len = bytes.len();
    if let Some(t) = trim {
        bytes.truncate(t as usize);
    }
    let ov = legaia_asset::summon_overlay::parse(&bytes, base);
    println!(
        "summon overlay {}: {} bytes ({} after trim), link base {:#010x}",
        input.display(),
        full_len,
        bytes.len(),
        ov.link_base
    );
    println!(
        "{} FUN_80021B04/FUN_80050ED4 spawn site(s), {} part record(s) recovered",
        ov.spawn_sites,
        ov.parts.len()
    );
    for (i, p) in ov.parts.iter().enumerate() {
        use legaia_asset::summon_overlay::SummonPartKind;
        let kind = match p.kind() {
            SummonPartKind::TransformNode => "transform-node".to_string(),
            SummonPartKind::LibraryMesh => format!("mesh-sel {}", p.model_sel),
            SummonPartKind::Sentinel => format!("sentinel {:#06x}", p.model_sel as u16),
        };
        println!(
            "  part {i:2}: rec @ file {:#06x} (rt {:#010x})  {kind}  flags {:#06x}  bytecode {:#x}..{:#x} ({} bytes)",
            p.record_off,
            base.wrapping_add(p.record_off as u32),
            p.flags,
            p.bytecode.start,
            p.bytecode.end,
            p.bytecode.len(),
        );
    }
    Ok(())
}

fn summon_readef_cmd(
    input: &Path,
    texture_png_dir: Option<&Path>,
    clut_sub: u8,
    action_id: Option<u8>,
) -> Result<()> {
    use legaia_asset::summon_readef::{self, SLOT_BYTES, SlotKind, StreamFile};

    if let Some(id) = action_id {
        let (file, slot) = summon_readef::stream_target(id);
        let (name, prot) = match file {
            StreamFile::Summon => ("summon.dat", summon_readef::SUMMON_PROT_INDEX),
            StreamFile::Readef => ("readef.DAT", summon_readef::READEF_PROT_INDEX),
        };
        println!(
            "action id {id:#04x}: base byte {:#04x} -> {name} (extraction PROT {prot:04}) slot {slot}",
            summon_readef::base_byte_for_action(id),
        );
        return Ok(());
    }

    let bytes = std::fs::read(input)?;
    let file = summon_readef::parse(&bytes)?;
    println!(
        "side-band file {}: {} bytes, {} slot(s) of {SLOT_BYTES:#x}",
        input.display(),
        bytes.len(),
        file.slots.len()
    );
    if let Some(dir) = texture_png_dir {
        std::fs::create_dir_all(dir)?;
    }
    if clut_sub > 15 {
        anyhow::bail!("--clut-sub must be 0..=15");
    }
    for slot in &file.slots {
        let raw = &bytes[slot.index * SLOT_BYTES..(slot.index + 1) * SLOT_BYTES];
        match &slot.kind {
            SlotKind::Texture(t) => {
                println!(
                    "  slot {:3}: texture  mode {}  {} CLUT row(s)  page {}x256 (4bpp)",
                    slot.index,
                    t.mode,
                    t.clut_rows,
                    t.texture_width_halfwords * 4,
                );
                if let Some(dir) = texture_png_dir {
                    let path = dir.join(format!("slot_{:03}.png", slot.index));
                    let (w, h) = (t.texture_width_halfwords * 4, 256usize);
                    // 16-color window of the first CLUT row (BGR555 at +4).
                    let clut_base = 4 + clut_sub as usize * 32;
                    let pal: Vec<[u8; 4]> = (0..16)
                        .map(|i| {
                            let off = clut_base + i * 2;
                            legaia_tim::bgr555_to_rgba8(u16::from_le_bytes([
                                raw[off],
                                raw[off + 1],
                            ]))
                        })
                        .collect();
                    let mut rgba = vec![0u8; w * h * 4];
                    for (texel, px) in rgba.chunks_exact_mut(4).enumerate() {
                        let byte = raw[t.texture_offset + texel / 2];
                        let idx = if texel % 2 == 0 {
                            byte & 0xF
                        } else {
                            byte >> 4
                        };
                        px.copy_from_slice(&pal[idx as usize]);
                    }
                    write_rgba_png(&path, w as u32, h as u32, &rgba)?;
                    println!("           -> {}", path.display());
                }
            }
            SlotKind::ActorRecord(r) => {
                println!(
                    "  slot {:3}: actor record  name {:?}  TMD @ +{:#06x}  pool @ +{:#06x}  {} part(s)",
                    slot.index,
                    r.name.as_deref().unwrap_or("-"),
                    r.tmd_offset,
                    r.texture_pool_offset,
                    r.part_count,
                );
            }
            SlotKind::MeArchive { count, compressed } => println!(
                "  slot {:3}: ME stream archive  {} entr{} ({} compressed)",
                slot.index,
                count,
                if *count == 1 { "y" } else { "ies" },
                compressed,
            ),
            SlotKind::Payload => println!("  slot {:3}: payload / raw", slot.index),
        }
    }
    Ok(())
}

/// `jr $ra` opcode (0x03E00008) in little-endian byte order.
const MIPS_JR_RA_LE: [u8; 4] = [0x08, 0x00, 0xE0, 0x03];

/// Test whether a 4-byte instruction word is `addiu $sp, $sp, -N`.
/// Encoding: 0x27BD_FFXX (low byte = -imm). LE bytes: [XX, FF, BD, 27].
fn is_sp_prologue(word: u32) -> bool {
    (word & 0xFFFF_0000) == 0x27BD_0000 && (word & 0x8000) != 0
}

/// Count word-aligned occurrences of `jr $ra`.
fn count_jr_ra(buf: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        if buf[i..i + 4] == MIPS_JR_RA_LE {
            n += 1;
        }
        i += 4;
    }
    n
}

/// Count word-aligned `addiu $sp, $sp, -N` instructions.
fn count_sp_prologue(buf: &[u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 4 <= buf.len() {
        let w = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        if is_sp_prologue(w) {
            n += 1;
        }
        i += 4;
    }
    n
}

/// Score a candidate buffer for "looks like MIPS code". Higher is better.
/// The signal is jr-ra and sp-prologue density per kilobyte, plus a soft
/// bonus when both are present.
fn code_score(buf: &[u8]) -> f32 {
    if buf.len() < 4096 {
        return 0.0;
    }
    let kb = buf.len() as f32 / 1024.0;
    let jr_ra = count_jr_ra(buf) as f32 / kb;
    let prologue = count_sp_prologue(buf) as f32 / kb;
    // Density caps prevent pathological repeated bytes from dominating.
    let s = jr_ra.min(5.0) + prologue.min(5.0);
    if jr_ra > 0.5 && prologue > 0.5 {
        s + 2.0
    } else {
        s
    }
}

fn find_overlay(dir: &PathBuf, top: usize, lzs_sizes: &str) -> Result<()> {
    let sizes: Vec<usize> = lzs_sizes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("bad --lzs-sizes: {e}"))?;

    #[derive(Clone)]
    struct Hit {
        path: PathBuf,
        size: usize,
        mode: String,
        decoded_size: usize,
        jr_ra: usize,
        prologue: usize,
        score: f32,
    }

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut hits: Vec<Hit> = Vec::new();
    let mut tried = 0usize;
    for path in &entries {
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if buf.len() < 4096 {
            continue;
        }
        tried += 1;

        // Raw scan.
        let s = code_score(&buf);
        if s > 0.3 {
            hits.push(Hit {
                path: path.clone(),
                size: buf.len(),
                mode: "raw".to_string(),
                decoded_size: buf.len(),
                jr_ra: count_jr_ra(&buf),
                prologue: count_sp_prologue(&buf),
                score: s,
            });
        }

        // LZS pass at file-start.
        for &out_sz in &sizes {
            if let Ok((decoded, _consumed)) = legaia_lzs::decompress_tracked(&buf, out_sz) {
                let s = code_score(&decoded);
                if s > 0.3 {
                    hits.push(Hit {
                        path: path.clone(),
                        size: buf.len(),
                        mode: format!("lzs@0+{out_sz}"),
                        decoded_size: decoded.len(),
                        jr_ra: count_jr_ra(&decoded),
                        prologue: count_sp_prologue(&decoded),
                        score: s,
                    });
                }
            }
        }

        // Sub-entry sweep: walk container offsets if the first u32 looks like
        // a small entry-count (player.lzs-style or TIM-pack-style), and try
        // LZS at each pointed-to offset. The runtime treats these the same
        // way -- each (size, offset) pair is independently LZS-decoded.
        if buf.len() >= 16 {
            let first = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
            // Heuristic count range covering every container we've seen so far.
            if (1..=64).contains(&first) {
                for i in 0..first {
                    let p = 4 + i * 4;
                    if p + 4 > buf.len() {
                        break;
                    }
                    let off = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap()) as usize;
                    if off >= buf.len() || off + 32 > buf.len() {
                        continue;
                    }
                    let sub = &buf[off..];
                    for &out_sz in &sizes {
                        if let Ok((decoded, _)) = legaia_lzs::decompress_tracked(sub, out_sz) {
                            let s = code_score(&decoded);
                            if s > 0.3 {
                                hits.push(Hit {
                                    path: path.clone(),
                                    size: buf.len(),
                                    mode: format!("lzs@0x{off:X}+{out_sz}"),
                                    decoded_size: decoded.len(),
                                    jr_ra: count_jr_ra(&decoded),
                                    prologue: count_sp_prologue(&decoded),
                                    score: s,
                                });
                            }
                        }
                        // Also try raw at this offset (for stored-uncompressed code).
                        if sub.len() >= 4096 {
                            let s = code_score(sub);
                            if s > 0.3 {
                                hits.push(Hit {
                                    path: path.clone(),
                                    size: buf.len(),
                                    mode: format!("raw@0x{off:X}"),
                                    decoded_size: sub.len(),
                                    jr_ra: count_jr_ra(sub),
                                    prologue: count_sp_prologue(sub),
                                    score: s,
                                });
                                break; // raw scoring doesn't depend on out_sz
                            }
                        }
                    }
                }
            }
        }
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(top);

    println!(
        "scanned {} files; {} candidates with score > 0.3",
        tried,
        hits.len()
    );
    println!(
        "{:>5} {:>9} {:>14} {:>9} {:>5} {:>5} {:>6}  path",
        "rank", "size", "mode", "out_size", "jr_ra", "prol", "score"
    );
    for (rank, h) in hits.iter().enumerate() {
        let name = h.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        println!(
            "{:>5} {:>9} {:>14} {:>9} {:>5} {:>5} {:>6.2}  {}",
            rank + 1,
            h.size,
            h.mode,
            h.decoded_size,
            h.jr_ra,
            h.prologue,
            h.score,
            name,
        );
    }
    Ok(())
}

fn describe(input: &PathBuf, count: usize) -> Result<()> {
    let raw = std::fs::read(input)?;
    let c = parse_player_lzs(&raw, count)?;
    println!("meta: 0x{:08X}, 0x{:08X}", c.meta[0], c.meta[1]);
    println!(
        "{:>3}  {:>4}  {:>9}  {:>10}  {:>10}",
        "i", "type", "size", "offset", "type_name"
    );
    for (i, d) in c.descriptors.iter().enumerate() {
        let t = d.asset_type();
        println!(
            "{:>3}  0x{:02X}  {:>9}  0x{:08X}  {}",
            i,
            d.type_byte,
            d.size,
            d.data_offset,
            t.name()
        );
    }
    Ok(())
}

fn decode_one(
    input: &PathBuf,
    type_size: u32,
    offset: u32,
    mode: ModeArg,
    out: Option<&PathBuf>,
) -> Result<()> {
    let raw = std::fs::read(input)?;
    let desc = Descriptor::from_pair(type_size, offset);
    let mode = match mode {
        ModeArg::Lzs => DecodeMode::Lzs,
        ModeArg::Raw => DecodeMode::Raw,
    };
    let decoded = decode(&raw, &desc, mode)?;
    eprintln!(
        "[ok] type={} size={} offset=0x{:X} → {} bytes",
        desc.asset_type().name(),
        desc.size,
        desc.data_offset,
        decoded.len()
    );
    match out {
        Some(p) => std::fs::write(p, &decoded)?,
        None => {
            let preview: String = decoded
                .iter()
                .take(64)
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            println!("{}", preview);
        }
    }
    Ok(())
}

fn scan(dir: &PathBuf, count: usize) -> Result<()> {
    let mut hits = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let raw = match std::fs::read(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let Ok(c) = parse_player_lzs(&raw, count) else {
            continue;
        };

        // Heuristic strictness: every descriptor must have a known type, a
        // sensible size (≥ 64 bytes, ≤ 4 MB), an offset past the header,
        // and decode cleanly under its declared mode (try LZS first, then raw).
        let header_end = (8 + count * 8) as u32;
        let valid_layout = c.descriptors.iter().all(|d| {
            !matches!(d.asset_type(), AssetType::Unknown(_))
                && (64..=4 * 1024 * 1024).contains(&d.size)
                && d.data_offset >= header_end
                && (d.data_offset as usize) < raw.len()
        });
        if !valid_layout {
            continue;
        }

        let mut all_ok = true;
        let mut total = 0usize;
        for d in &c.descriptors {
            let r = decode(&raw, d, DecodeMode::Lzs).or_else(|_| decode(&raw, d, DecodeMode::Raw));
            match r {
                Ok(v) => total += v.len(),
                Err(_) => {
                    all_ok = false;
                    break;
                }
            }
        }
        if all_ok {
            hits += 1;
            println!(
                "{}  meta=[0x{:X},0x{:X}]  descriptors={}  total_decoded={}b",
                path.file_name().unwrap_or_default().to_string_lossy(),
                c.meta[0],
                c.meta[1],
                c.descriptors.len(),
                total
            );
        }
    }
    eprintln!("scan done: {} hits", hits);
    Ok(())
}

fn parse_hex_u32(s: &str) -> std::result::Result<u32, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).map_err(|e| e.to_string())
}

fn worldmap_menu_cmd(scus: &Path, json: bool) -> Result<()> {
    let bytes = std::fs::read(scus)?;
    let menu = legaia_asset::worldmap_menu::parse_scus(&bytes)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&menu)?);
        return Ok(());
    }
    println!(
        "World-map quick-travel menu  ({} names, {} placement records)\n",
        menu.names.len(),
        menu.placements.len(),
    );
    println!("Names (DAT_80073B18, stride 0x20):");
    for (i, name) in menu.names.iter().enumerate() {
        let used = menu.placements.iter().any(|p| (p.name_idx as usize) == i);
        let tag = if used { "  " } else { "* " };
        println!("  {tag}[0x{i:02X}] {name:?}");
    }
    println!("* = not referenced by any placement record (cutscene-only).\n");
    println!(
        "Placements (DAT_80073A98, stride 6; terminator byte0=0xFF):\n  \
         idx flag scene_id  menu_xy   name"
    );
    for p in &menu.placements {
        let name = menu
            .names
            .get(p.name_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("<?>");
        println!(
            "   {:>2}  0x{:02X}  0x{:04X}   ({:3}, {:3})  {}",
            p.index, p.discovery_flag, p.scene_id, p.menu_x, p.menu_y, name
        );
    }
    Ok(())
}

fn item_tables_cmd(scus: &Path, equipment_only: bool, consumables_only: bool) -> Result<()> {
    use legaia_asset::{equip_stats, item_effect, item_names};

    let bytes = std::fs::read(scus)?;
    let names = item_names::ItemNameTable::from_scus(&bytes).context("parse item-name table")?;
    let effects =
        item_effect::ItemEffectTable::from_scus(&bytes).context("parse item-effect table")?;
    let equips =
        equip_stats::EquipStatTable::from_scus(&bytes).context("parse equip-stat table")?;

    println!("id    name                       category");
    for id in 0u8..=u8::MAX {
        let name = names.name(id).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        if let Some(b) = equips.bonus(id) {
            if consumables_only {
                continue;
            }
            let bonuses = b.stat_bonus();
            let ra = if b.is_ra_seru() { " ra-seru" } else { "" };
            println!(
                "0x{id:02X}  {name:26} equip  atk={} udf={} ldf={} mask={:#05b} slot={:?}{ra} \
                 [+0={} +4={}]",
                b.attack(),
                b.def_up(),
                b.def_down(),
                b.equip_mask(),
                b.slot(),
                bonuses[0],
                bonuses[4],
            );
        } else if let Some(e) = effects.effect(id) {
            if equipment_only || !e.is_usable_consumable() {
                continue;
            }
            let mut where_ = String::new();
            if e.field_usable() {
                where_.push('F');
            }
            if e.battle_usable() {
                where_.push('B');
            }
            if e.all_party() {
                where_.push_str(" all-party");
            }
            println!(
                "0x{id:02X}  {name:26} {:?} tier={} [{}]",
                e.category(),
                e.tier,
                where_,
            );
        }
    }
    Ok(())
}

/// `asset spell-names <SCUS>` - dump the static spell name / MP / target
/// table (`legaia_asset::spell_names`, `DAT_800754C8`).
fn spell_names_cmd(scus: &Path) -> Result<()> {
    use legaia_asset::spell_names::SpellNameTable;

    let bytes = std::fs::read(scus)?;
    let table = SpellNameTable::from_scus(&bytes).context("parse spell-name table")?;
    let mut named = 0usize;
    println!("id    mp   target           name");
    for id in 0u8..=u8::MAX {
        let Some(e) = table.entry(id) else { continue };
        let name = e.name.as_deref().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        named += 1;
        println!("0x{id:02X}  {:<4} {:<16?} {name}", e.mp, e.target_shape());
    }
    println!("\n{named} named spell ids");
    Ok(())
}

/// `asset steal-table <SCUS>` - dump the static per-monster steal table
/// (`legaia_asset::steal_table`, `DAT_80077828`), joining the stolen item
/// id to its name from the item-name table.
fn steal_table_cmd(scus: &Path, all: bool) -> Result<()> {
    use legaia_asset::{item_names::ItemNameTable, steal_table::StealTable};

    let bytes = std::fs::read(scus)?;
    let table = StealTable::from_scus(&bytes).context("parse steal table")?;
    let names = ItemNameTable::from_scus(&bytes);
    println!("monster  chance  item");
    for monster_id in 1u16..=255 {
        let Some(e) = table.entry(monster_id) else {
            continue;
        };
        if !all && !e.is_stealable() {
            continue;
        }
        let item = names.as_ref().and_then(|n| n.name(e.item_id)).unwrap_or("");
        println!(
            "{monster_id:>5}    {:>3}%    0x{:02X} {item}",
            e.chance_pct, e.item_id
        );
    }
    println!(
        "\n{} stealable of {} entries",
        table.stealable_count(),
        table.len()
    );
    Ok(())
}

/// `asset accessory-passive <SCUS>` - dump the 64-slot accessory ("Goods")
/// passive-effect table (`legaia_asset::accessory_passive`, `0x8007625C`).
fn accessory_passive_cmd(scus: &Path) -> Result<()> {
    use legaia_asset::accessory_passive::{AccessoryPassiveTable, stat_boosts};

    let bytes = std::fs::read(scus)?;
    let table =
        AccessoryPassiveTable::from_scus(&bytes).context("parse accessory-passive table")?;
    println!("idx   scope  name                          boosts / effect");
    for i in 0..table.record_count() {
        let idx = i as u8;
        let Some(rec) = table.record(idx) else {
            continue;
        };
        let name = rec.name.as_deref().unwrap_or("");
        let scope = if rec.party_wide() { "party" } else { "self " };
        let boosts = stat_boosts(idx);
        let effect = if boosts.is_empty() {
            String::new()
        } else {
            boosts
                .iter()
                .map(|(s, p)| format!("{s:?}+{p}%"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!("0x{idx:02X}  {scope}  {name:28}  {effect}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn slot4_png_cmd(
    input: Option<&Path>,
    from_raw: Option<&Path>,
    out: &Path,
    placements_path: Option<&Path>,
    kingdom: &str,
    width: u32,
    height: u32,
    margin: u32,
    only_body: Option<usize>,
    frame_body: Option<usize>,
    close_polylines: bool,
    style: &str,
    axes: &str,
) -> Result<()> {
    use legaia_asset::{kingdom_bundle, world_map_overlay};

    if input.is_some() == from_raw.is_some() {
        anyhow::bail!("exactly one of --input or --from-raw is required");
    }
    let mode = match style {
        "row" => world_map_overlay::PolylineMode::RowMajor,
        "col" => world_map_overlay::PolylineMode::ColumnMajor,
        "pairs" => world_map_overlay::PolylineMode::PairWise,
        "grid" => world_map_overlay::PolylineMode::Grid,
        "points" => world_map_overlay::PolylineMode::RowMajor, // mode ignored in points-only path
        other => anyhow::bail!("--style must be row|col|pairs|grid|points (got {other})"),
    };
    let points_only = style == "points";
    let parse_axis = |c: char| match c {
        'x' | 'X' => Ok(world_map_overlay::Axis::X),
        'y' | 'Y' => Ok(world_map_overlay::Axis::Y),
        'z' | 'Z' => Ok(world_map_overlay::Axis::Z),
        other => anyhow::bail!("axis '{other}' must be one of x|y|z"),
    };
    let chars: Vec<char> = axes.chars().collect();
    if chars.len() != 2 {
        anyhow::bail!("--axes must be 2 chars from x|y|z (got '{axes}')");
    }
    let axis_pair = (parse_axis(chars[0])?, parse_axis(chars[1])?);

    // Source the decoded slot-4 bytes from either a kingdom PROT entry or
    // a previously-decoded .bin.
    let decoded: Vec<u8> = if let Some(p) = input {
        let buf = std::fs::read(p)?;
        kingdom_bundle::decode_slot(&buf, 4)
            .map_err(|e| anyhow::anyhow!("decode slot 4 from {p:?}: {e}"))?
    } else {
        std::fs::read(from_raw.unwrap())?
    };

    let parsed =
        world_map_overlay::parse(&decoded).map_err(|e| anyhow::anyhow!("parse slot 4: {e}"))?;
    println!(
        "Parsed slot 4: {} bodies, {} bytes decoded",
        parsed.bodies.len(),
        decoded.len()
    );

    let opts = world_map_overlay::WireframeOptions {
        close_polylines,
        mode,
        axes: axis_pair,
        ..world_map_overlay::WireframeOptions::default()
    };
    if points_only {
        let pts = world_map_overlay::record_points(&parsed, &opts);
        println!("Record points: {}", pts.len());
    } else {
        let lines = world_map_overlay::top_down_lines(&parsed, &opts);
        println!("Top-down line segments: {}", lines.len());
    }

    let mut raster =
        world_map_overlay::WireframeRaster::new(width, height, margin, [0x0A, 0x0A, 0x1A, 0xFF]);
    let (ah, av) = axis_pair;
    if let Some(b) = frame_body {
        let body = parsed
            .bodies
            .get(b)
            .ok_or_else(|| anyhow::anyhow!("--frame-body {b} out of range"))?;
        let mut amin = i16::MAX;
        let mut bmin = i16::MAX;
        let mut amax = i16::MIN;
        let mut bmax = i16::MIN;
        for r in &body.records {
            if r.x == 0 && r.y == 0 && r.z == 0 {
                continue;
            }
            let a = ah.pick(r);
            let v = av.pick(r);
            amin = amin.min(a);
            bmin = bmin.min(v);
            amax = amax.max(a);
            bmax = bmax.max(v);
        }
        if amin == i16::MAX {
            anyhow::bail!("--frame-body {b} has no non-zero records");
        }
        raster.set_bounds(amin as i32, bmin as i32, amax as i32, bmax as i32);
    } else {
        raster.set_bounds_from_axes(&parsed, ah, av);
    }
    let (amin, bmin, amax, bmax) = raster.world_bounds;
    println!("Camera bounds ({axes}): {amin}..{amax}, {bmin}..{bmax}");

    if points_only {
        raster.draw_points(&parsed, &opts, only_body, 1);
    } else {
        raster.draw_wireframe(&parsed, &opts, only_body);
    }

    if let Some(pp) = placements_path {
        match load_placements(pp, kingdom) {
            Ok(pts) => {
                println!(
                    "Overlaying {} placements for kingdom '{kingdom}'",
                    pts.len()
                );
                // Placement coords use a different scale than slot-4 (placements
                // are in `[0, world_extent]` while slot-4 is in centered ±32K).
                // We map placements into the current camera's bbox so a dot's
                // RELATIVE position within the kingdom carries over - imperfect
                // but enough for "does landmark N sit roughly inside the
                // is-this-anything?" eyeballing.
                let (xmin, zmin, xmax, zmax) = raster.world_bounds;
                let mut pmin_x = i32::MAX;
                let mut pmin_z = i32::MAX;
                let mut pmax_x = i32::MIN;
                let mut pmax_z = i32::MIN;
                for &(x, z) in &pts {
                    pmin_x = pmin_x.min(x);
                    pmin_z = pmin_z.min(z);
                    pmax_x = pmax_x.max(x);
                    pmax_z = pmax_z.max(z);
                }
                let dx_p = (pmax_x - pmin_x).max(1) as f32;
                let dz_p = (pmax_z - pmin_z).max(1) as f32;
                let dx_w = (xmax - xmin).max(1) as f32;
                let dz_w = (zmax - zmin).max(1) as f32;
                let mapped: Vec<(i32, i32)> = pts
                    .iter()
                    .map(|&(x, z)| {
                        let nx = (x - pmin_x) as f32 / dx_p;
                        let nz = (z - pmin_z) as f32 / dz_p;
                        let mx = (nx * dx_w) as i32 + xmin;
                        let mz = (nz * dz_w) as i32 + zmin;
                        (mx, mz)
                    })
                    .collect();
                raster.draw_placements(&mapped, [0xF4, 0xB4, 0x1A, 0xFF], 3);
            }
            Err(e) => {
                eprintln!("warn: skipping placement overlay ({e})");
            }
        }
    }

    let f = std::fs::File::create(out)?;
    raster
        .encode_png(std::io::BufWriter::new(f))
        .map_err(|e| anyhow::anyhow!("write PNG: {e}"))?;
    println!("Wrote {out:?}  ({width}x{height})");
    Ok(())
}

/// Tiny JSON-ish picker for the `world-overview.json` placement records.
/// Returns `Vec<(x, z)>` in world units, filtering out script-positioned
/// records (which carry no static world coordinate). We hand-roll the
/// extraction to avoid pulling a full serde_json model just for two ints.
fn load_placements(path: &Path, kingdom: &str) -> Result<Vec<(i32, i32)>> {
    let raw = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let king = value
        .get(kingdom)
        .ok_or_else(|| anyhow::anyhow!("kingdom '{kingdom}' not in placement JSON"))?;
    let arr = king
        .get("placements")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow::anyhow!("no `placements` array under '{kingdom}'"))?;
    let mut pts = Vec::new();
    for p in arr {
        if p.get("script_positioned").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let pos = p.get("pos").and_then(|v| v.as_array());
        if let Some(a) = pos
            && a.len() >= 3
            && let (Some(x), Some(z)) = (a[0].as_i64(), a[2].as_i64())
        {
            pts.push((x as i32, z as i32));
        }
    }
    Ok(pts)
}

fn kingdom_slot_cmd(
    input: &Path,
    slot: u8,
    out: Option<&Path>,
    wireframe_obj: Option<&Path>,
) -> Result<()> {
    use legaia_asset::{kingdom_bundle, world_map_overlay};

    let buf = std::fs::read(input)?;
    let bundle = kingdom_bundle::parse(&buf).ok_or_else(|| {
        anyhow::anyhow!("no 7-asset table found at any 0x800-aligned offset in {input:?}")
    })?;
    println!("PROT entry: {} bytes", buf.len());
    println!("Asset table at 0x{:X}", bundle.table_offset);
    println!();
    println!(
        "{:<5}  {:<8}  {:>12}  {:>10}  {:>10}",
        "Slot", "Type", "Declared size", "Data off", "Decoded"
    );
    println!("{}", "-".repeat(64));
    for s in &bundle.slots {
        let decoded_n = match &s.decoded {
            Ok(b) => b.len() as i64,
            Err(_) => -1,
        };
        let decoded_str = if decoded_n >= 0 {
            format!("{decoded_n} OK")
        } else {
            "(LZS err)".to_string()
        };
        println!(
            "{:<5}  0x{:02X}    {:>12}   0x{:08X}  {:>10}",
            s.index, s.type_byte, s.declared_size, s.data_offset, decoded_str
        );
    }
    println!();

    let target = bundle
        .slots
        .iter()
        .find(|s| s.index == slot)
        .ok_or_else(|| anyhow::anyhow!("slot {slot} not present"))?;
    let bytes = match &target.decoded {
        Ok(b) => b.clone(),
        Err(e) => anyhow::bail!("slot {slot}: LZS decode failed: {e}"),
    };
    println!(
        "Selected slot {slot}: type 0x{:02X}, {} decoded bytes",
        target.type_byte,
        bytes.len()
    );

    if let Some(path) = out {
        std::fs::write(path, &bytes)?;
        println!("  wrote raw decoded bytes -> {path:?}");
    }

    if slot == 4 {
        match world_map_overlay::parse(&bytes) {
            Ok(parsed) => {
                println!();
                println!("World-map slot-4 container: {} bodies", parsed.bodies.len());
                println!(
                    "{:<6}  {:>6}  {:>6}  {:>5}  {:>4}  {:>6}  {:>9}",
                    "Body", "ca", "cb", "kind", "flag", "recs", "non-zero"
                );
                println!("{}", "-".repeat(60));
                for b in &parsed.bodies {
                    let nz = b
                        .records
                        .iter()
                        .filter(|r| !(r.x == 0 && r.y == 0 && r.z == 0))
                        .count();
                    println!(
                        "{:<6}  {:>6}  {:>6}  {:>5}  {:>2},{}  {:>6}  {:>9}",
                        b.index,
                        b.count_a,
                        b.count_b,
                        b.kind,
                        b.flag_a,
                        b.flag_b,
                        b.records.len(),
                        nz
                    );
                }
                if let Some((xmin, zmin, xmax, zmax)) = world_map_overlay::xz_bounds(&parsed) {
                    println!(
                        "\nTop-down (X-Z) bounds (non-zero records): \
                         x = {xmin}..{xmax}, z = {zmin}..{zmax}"
                    );
                }
                if let Some(obj_path) = wireframe_obj {
                    let opts = world_map_overlay::WireframeOptions::default();
                    let lines = world_map_overlay::top_down_lines(&parsed, &opts);
                    write_wireframe_obj(obj_path, &lines)?;
                    println!("  wrote {} line segments -> {obj_path:?}", lines.len());
                }
            }
            Err(e) => {
                println!("\nslot 4 parse failed: {e}");
            }
        }
    }
    Ok(())
}

/// Write a wireframe-only Wavefront OBJ (X-Z plane, vertices use Y=0).
/// Each line becomes two vertices + one `l` directive. OBJ indices start
/// at 1.
fn write_wireframe_obj(
    path: &Path,
    lines: &[legaia_asset::world_map_overlay::WireframeLine],
) -> Result<()> {
    let mut s = String::new();
    s.push_str("# slot-4 wireframe (top-down X-Z)\n");
    s.push_str(&format!("# {} line segments\n", lines.len()));
    for l in lines {
        s.push_str(&format!("v {} 0 {}\n", l.x0, l.z0));
        s.push_str(&format!("v {} 0 {}\n", l.x1, l.z1));
    }
    for (i, _) in lines.iter().enumerate() {
        let a = 2 * i + 1;
        let b = 2 * i + 2;
        s.push_str(&format!("l {a} {b}\n"));
    }
    std::fs::write(path, s)?;
    Ok(())
}

fn validate_blocks(
    dir: &PathBuf,
    cdname_path: Option<&PathBuf>,
    counts_str: &str,
    only_hits: bool,
) -> Result<()> {
    let counts: Vec<usize> = counts_str
        .split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("invalid --counts: {}", e))?;

    // Build a name lookup table from `<index>_<name>.BIN` filenames produced
    // by prot-extract. Index → full path.
    let mut index_to_path: std::collections::BTreeMap<u32, PathBuf> = Default::default();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((idx_str, _)) = stem.split_once('_') else {
            continue;
        };
        let Ok(idx) = idx_str.parse::<u32>() else {
            continue;
        };
        index_to_path.insert(idx, p);
    }

    // Pick which entries to test: CDNAME block heads, or all entries.
    let test_indices: Vec<(u32, String)> = if let Some(p) = cdname_path {
        let map = cdname::parse(p)?;
        map.into_iter().collect()
    } else {
        index_to_path
            .keys()
            .map(|&i| (i, format!("entry_{:04}", i)))
            .collect()
    };

    let mut hits = 0usize;
    let mut tried = 0usize;
    for (start_idx, block_name) in &test_indices {
        let Some(path) = index_to_path.get(start_idx) else {
            continue;
        };
        tried += 1;
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Pick the best count: highest one that yields layout_ok with at
        // least one descriptor decoding cleanly to a known magic OR all
        // descriptors decoding without error.
        let mut best: Option<(usize, legaia_asset::ContainerReport)> = None;
        for &n in &counts {
            if raw.len() < 8 + n * 8 {
                continue;
            }
            let report = match validate(&raw, n) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let any_magic_ok = report
                .descriptors
                .iter()
                .any(|d| d.magic_ok && d.decoded_as.is_some());
            let all_decoded = report
                .descriptors
                .iter()
                .all(|d| d.decoded_as.is_some() || d.error.is_some() && !report.layout_ok);
            // Prefer reports with layout_ok and a real magic hit.
            let score =
                (report.layout_ok as u8) * 4 + (any_magic_ok as u8) * 2 + (all_decoded as u8);
            let prev_score = best.as_ref().map(|(_, r)| {
                (r.layout_ok as u8) * 4
                    + (r.descriptors
                        .iter()
                        .any(|d| d.magic_ok && d.decoded_as.is_some()) as u8)
                        * 2
            });
            if prev_score.is_none_or(|ps| score > ps) {
                best = Some((n, report));
            }
        }

        let Some((count, report)) = best else {
            if !only_hits {
                println!(
                    "[skip] block={} idx={} {}: no count fits",
                    block_name,
                    start_idx,
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
            }
            continue;
        };

        let any_magic_ok = report
            .descriptors
            .iter()
            .any(|d| d.magic_ok && d.decoded_as.is_some());
        let is_hit = report.layout_ok && any_magic_ok;
        if !is_hit && only_hits {
            continue;
        }
        if is_hit {
            hits += 1;
        }
        let tag = if is_hit { "HIT " } else { "miss" };
        println!(
            "{}  block={:<16} idx={:>4}  count={}  layout_ok={}  file={}",
            tag,
            block_name,
            start_idx,
            count,
            report.layout_ok,
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        for d in &report.descriptors {
            let mode = d.decoded_as.unwrap_or("--");
            let mag = d.decoded_magic.as_deref().unwrap_or("        ");
            let magic_tag = if d.magic_ok { "OK " } else { "?? " };
            let len = d
                .decoded_len
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into());
            let err = d.error.as_deref().unwrap_or("");
            println!(
                "    [{:>2}] type=0x{:02X} {:>8}  size={:>8}  off=0x{:08X}  mode={:<3}  magic={} {}  decoded={:>8}  {}",
                d.index,
                d.type_byte,
                d.type_name,
                d.size,
                d.data_offset,
                mode,
                mag,
                magic_tag,
                len,
                err
            );
        }
    }
    eprintln!("validate done: {} blocks tested, {} hits", tried, hits);
    Ok(())
}

fn categorize_dir(
    dir: &PathBuf,
    out: Option<&PathBuf>,
    top_signatures: usize,
    examples: usize,
    filter_class: Option<&str>,
    cdname_path: Option<&Path>,
) -> Result<()> {
    use std::collections::BTreeMap;

    #[derive(serde::Serialize)]
    struct PerFile<'a> {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cdname_slot: Option<String>,
        #[serde(flatten)]
        report: &'a categorize::FileReport,
    }

    #[derive(serde::Serialize)]
    struct ClassBucket<'a> {
        class: &'static str,
        count: usize,
        total_bytes: usize,
        examples: Vec<&'a String>,
    }

    #[derive(serde::Serialize)]
    struct SignatureBucket {
        first_u32_hex: String,
        count: usize,
        examples: Vec<String>,
    }

    #[derive(serde::Serialize)]
    struct SlotHistogramRow {
        slot: u32,
        count: usize,
        scene_examples: Vec<String>,
    }

    #[derive(serde::Serialize)]
    struct Report<'a> {
        scan_root: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filter_class: Option<&'a str>,
        n_files: usize,
        per_file: Vec<PerFile<'a>>,
        by_class: Vec<ClassBucket<'a>>,
        top_signatures: Vec<SignatureBucket>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        slot_histogram: Vec<SlotHistogramRow>,
    }

    // Load CDNAME map if requested.
    let cdname_map = cdname_path
        .map(cdname::parse)
        .transpose()
        .map_err(|e| anyhow::anyhow!("CDNAME parse error: {e}"))?;

    // Helper: PROT entry index from a filename like `0042_scene.BIN`.
    let entry_index_from_name =
        |name: &str| -> Option<u32> { name.split('_').next()?.parse::<u32>().ok() };

    // Helper: resolve a PROT entry index to `scene+slot` or `raw_N`.
    let slot_label = |idx: u32| -> String {
        if let Some(ref map) = cdname_map
            && let Some(scene) = cdname::block_for(map, idx)
        {
            // Find the scene start index to compute the slot offset.
            let start = map.range(..=idx).next_back().map(|(k, _)| *k).unwrap_or(0);
            return format!("{}+{}", scene, idx - start);
        }
        format!("raw_{}", idx)
    };

    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    let mut reports: Vec<categorize::FileReport> = Vec::with_capacity(paths.len());
    let mut names: Vec<String> = Vec::with_capacity(paths.len());
    let mut slot_labels: Vec<Option<String>> = Vec::with_capacity(paths.len());

    for p in &paths {
        let buf = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {}: {}", p.display(), e);
                continue;
            }
        };
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string());
        reports.push(categorize::classify(&buf));
        let lbl = if cdname_map.is_some() {
            entry_index_from_name(&name).map(&slot_label)
        } else {
            None
        };
        slot_labels.push(lbl);
        names.push(name);
    }

    let n_files = reports.len();

    // Group by class (unfiltered, for the summary table).
    let mut by_class: BTreeMap<&'static str, (usize, usize, Vec<&String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let entry = by_class.entry(r.class.name()).or_insert((0, 0, Vec::new()));
        entry.0 += 1;
        entry.1 += r.size;
        if entry.2.len() < examples {
            entry.2.push(&names[i]);
        }
    }

    // Slot histogram (only for filtered class when --cdname is given).
    let mut slot_hist: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
    if cdname_map.is_some() {
        for (i, r) in reports.iter().enumerate() {
            let matches_filter = filter_class.map(|f| r.class.name() == f).unwrap_or(true);
            if !matches_filter {
                continue;
            }
            if let Some(idx) = entry_index_from_name(&names[i]) {
                // slot offset within the scene block
                let slot_offset = if let Some(ref map) = cdname_map {
                    let start = map.range(..=idx).next_back().map(|(k, _)| *k).unwrap_or(0);
                    idx - start
                } else {
                    idx
                };
                let entry = slot_hist.entry(slot_offset).or_insert((0, Vec::new()));
                entry.0 += 1;
                if entry.1.len() < 3 {
                    entry.1.push(names[i].clone());
                }
            }
        }
    }
    let mut slot_histogram: Vec<SlotHistogramRow> = slot_hist
        .into_iter()
        .map(|(slot, (count, ex))| SlotHistogramRow {
            slot,
            count,
            scene_examples: ex,
        })
        .collect();
    slot_histogram.sort_by(|a, b| b.count.cmp(&a.count).then(a.slot.cmp(&b.slot)));

    // Group by first-u32 signature (filtered).
    let mut by_sig: BTreeMap<u32, (usize, Vec<String>)> = BTreeMap::new();
    for (i, r) in reports.iter().enumerate() {
        let matches_filter = filter_class.map(|f| r.class.name() == f).unwrap_or(true);
        if !matches_filter {
            continue;
        }
        let Some(sig) = r.first_u32 else { continue };
        let entry = by_sig.entry(sig).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(names[i].clone());
        }
    }
    let mut sigs: Vec<SignatureBucket> = by_sig
        .into_iter()
        .map(|(s, (c, ex))| SignatureBucket {
            first_u32_hex: format!("0x{:08X}", s),
            count: c,
            examples: ex,
        })
        .collect();
    sigs.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.first_u32_hex.cmp(&b.first_u32_hex))
    });
    sigs.truncate(top_signatures);

    let class_buckets: Vec<ClassBucket> = by_class
        .iter()
        .map(|(name, (c, b, ex))| ClassBucket {
            class: name,
            count: *c,
            total_bytes: *b,
            examples: ex.clone(),
        })
        .collect();

    // Console summary (unfiltered class table).
    let filter_note = filter_class
        .map(|f| format!(" (filter: {f})"))
        .unwrap_or_default();
    println!("=== categorize: {} files{} ===", n_files, filter_note);
    println!();
    println!(
        "{:>5}  {:>9}  class                      examples",
        "n", "MB"
    );
    let mut sorted_classes: Vec<_> = by_class.iter().collect();
    sorted_classes.sort_by_key(|b| std::cmp::Reverse(b.1.0));
    for (name, (count, total, ex)) in &sorted_classes {
        let marker = if filter_class == Some(name) {
            " <--"
        } else {
            ""
        };
        let mb = (*total as f64) / (1024.0 * 1024.0);
        let ex_str = ex
            .iter()
            .take(3)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:>5}  {:>9.2}  {:<26} {}{}",
            count, mb, name, ex_str, marker
        );
    }
    println!();

    // Per-file listing for the filtered class (when filter_class is given).
    if let Some(fc) = filter_class {
        println!("=== entries matching class '{}' ===", fc);
        let mut printed = 0usize;
        for (i, r) in reports.iter().enumerate() {
            if r.class.name() != fc {
                continue;
            }
            let lbl = slot_labels[i].as_deref().unwrap_or("");
            let lbl_col = if lbl.is_empty() {
                String::new()
            } else {
                format!("  [{}]", lbl)
            };
            println!(
                "  {:>9}B  h={:.2}  head={}{}  {}",
                r.size, r.entropy_bits, r.head, lbl_col, names[i]
            );
            printed += 1;
        }
        println!("  ({} entries)", printed);
        println!();
    }

    if !slot_histogram.is_empty() {
        println!(
            "=== slot histogram (class '{}') ===",
            filter_class.unwrap_or("all")
        );
        println!("{:>5}  {:>5}  scene examples", "slot", "count");
        for row in &slot_histogram {
            let ex = row.scene_examples.join(", ");
            println!("{:>5}  {:>5}  {}", row.slot, row.count, ex);
        }
        println!();
    }

    println!("=== top {} first-u32 signatures ===", sigs.len());
    println!("{:>5}  {:<12}  examples", "n", "signature");
    for sb in &sigs {
        let ex = sb
            .examples
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!("{:>5}  {:<12}  {}", sb.count, sb.first_u32_hex, ex);
    }

    let per_file: Vec<PerFile> = reports
        .iter()
        .zip(names.iter())
        .zip(slot_labels.iter())
        .filter(|((r, _), _)| filter_class.map(|f| r.class.name() == f).unwrap_or(true))
        .map(|((r, name), lbl)| PerFile {
            path: name.clone(),
            cdname_slot: lbl.clone(),
            report: r,
        })
        .collect();

    let report = Report {
        scan_root: dir.display().to_string(),
        filter_class,
        n_files,
        per_file,
        by_class: class_buckets,
        top_signatures: sigs,
        slot_histogram,
    };

    let out_path: PathBuf = out.cloned().unwrap_or_else(|| dir.join("categorize.json"));
    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&out_path, json)?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}

fn stream_one(input: &PathBuf, max_chunks: usize) -> Result<()> {
    let raw = std::fs::read(input)?;
    let r = parse_streaming(&raw, max_chunks)?;
    println!(
        "chunks={}  terminated={}  all_known_types={}  all_magic_ok={}  bytes_consumed={} / {}",
        r.chunks.len(),
        r.terminated,
        r.all_known_types,
        r.all_magic_ok,
        r.bytes_consumed,
        raw.len()
    );
    println!(
        "{:>3}  {:>4}  {:>9}  {:>10}  {:>9}  magic_ok",
        "i", "type", "size", "off", "name"
    );
    for (i, c) in r.chunks.iter().enumerate() {
        println!(
            "{:>3}  0x{:02X}  {:>9}  0x{:08X}  {:>9}  {} {}",
            i,
            c.type_byte,
            c.size,
            c.header_offset,
            c.type_name,
            c.magic,
            if c.magic_ok { "ok" } else { "MISMATCH" },
        );
    }
    Ok(())
}

fn scan_stream(dir: &PathBuf, max_chunks: usize, only_hits: bool, min_chunks: usize) -> Result<()> {
    let mut hits = 0usize;
    let mut tried = 0usize;
    // Sort by filename so output is stable.
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    for path in &paths {
        tried += 1;
        let raw = match std::fs::read(path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let report = match parse_streaming(&raw, max_chunks) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // A "hit" is: terminated cleanly, all types known, all magics ok,
        // and at least min_chunks chunks (so empty/junk doesn't count).
        let is_hit = report.terminated
            && report.all_known_types
            && report.all_magic_ok
            && report.chunks.len() >= min_chunks;
        if !is_hit && only_hits {
            continue;
        }
        if is_hit {
            hits += 1;
        }
        let tag = if is_hit { "HIT " } else { "miss" };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        println!(
            "{}  {}  chunks={:<3} terminated={} types_ok={} magic_ok={} bytes={}/{}",
            tag,
            name,
            report.chunks.len(),
            report.terminated,
            report.all_known_types,
            report.all_magic_ok,
            report.bytes_consumed,
            raw.len()
        );
        if is_hit {
            for c in report.chunks.iter().take(8) {
                println!(
                    "      [{}] type=0x{:02X} {:<8}  size={:>8}  magic={} {}",
                    c.header_offset,
                    c.type_byte,
                    c.type_name,
                    c.size,
                    c.magic,
                    if c.magic_ok { "ok" } else { "??" },
                );
            }
            if report.chunks.len() > 8 {
                println!("      ... +{} more", report.chunks.len() - 8);
            }
        }
    }
    eprintln!("scan-stream done: {} entries tested, {} hits", tried, hits);
    Ok(())
}

fn detect_extension(asset_type: AssetType, data: &[u8]) -> &'static str {
    // Pre-empt by content first: a TIM always starts with 0x00000010.
    if data.len() >= 4 && u32::from_le_bytes(data[..4].try_into().unwrap()) == 0x0000_0010 {
        return "tim";
    }
    match asset_type {
        AssetType::Tim => "tim",
        AssetType::TimList => "tim",
        AssetType::Tmd | AssetType::Tmd2 => "tmd",
        _ => "bin",
    }
}

fn extract_streaming(input: &PathBuf, out: &PathBuf, save_trailer: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let report = parse_streaming(&raw, 4096)?;
    if !report.terminated {
        eprintln!(
            "[warn] streaming parse did not hit terminator (consumed {}/{})",
            report.bytes_consumed,
            raw.len()
        );
    }

    std::fs::create_dir_all(out)?;
    let mut total_subassets = 0usize;
    for (chunk_idx, c) in report.chunks.iter().enumerate() {
        let chunk_data_start = c.header_offset + 4;
        let chunk_data_end = chunk_data_start + c.size as usize;
        let chunk_data = &raw[chunk_data_start..chunk_data_end];
        let t = AssetType::from_byte(c.type_byte);

        // Decide: pack-style (TIM_LIST/TMD) or single-asset.
        // TMD2 (case 9 in FUN_8001f05c) is a *bare* TMD - the dispatcher passes
        // the buffer directly to FUN_80026b4c without walking a pack header.
        // Case 2 (TMD), by contrast, walks `puVar1[i]` as pack offsets.
        let is_pack = matches!(t, AssetType::TimList | AssetType::Tmd);
        let chunk_dir = out.join(format!("chunk{:02}_{}", chunk_idx, t.name()));
        std::fs::create_dir_all(&chunk_dir)?;

        if is_pack {
            match legaia_asset::pack::extract_pack(chunk_data) {
                Ok(items) => {
                    println!(
                        "chunk @ 0x{:08X}  type={:<8}  size={:>9}  ->  {} sub-assets",
                        c.header_offset,
                        t.name(),
                        c.size,
                        items.len()
                    );
                    for (j, item) in items.iter().enumerate() {
                        let ext = detect_extension(t, item);
                        let path = chunk_dir.join(format!("{:04}.{}", j, ext));
                        std::fs::write(&path, item)?;
                        total_subassets += 1;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[warn] chunk @ 0x{:X} ({}, size={}) is not a valid pack: {}",
                        c.header_offset,
                        t.name(),
                        c.size,
                        e
                    );
                    let path = chunk_dir.join("raw.bin");
                    std::fs::write(&path, chunk_data)?;
                    total_subassets += 1;
                }
            }
        } else {
            let ext = detect_extension(t, chunk_data);
            let path = chunk_dir.join(format!("0000.{}", ext));
            std::fs::write(&path, chunk_data)?;
            total_subassets += 1;
            println!(
                "chunk @ 0x{:08X}  type={:<8}  size={:>9}  ->  raw.{}",
                c.header_offset,
                t.name(),
                c.size,
                ext
            );
        }
    }

    if save_trailer && report.bytes_consumed < raw.len() {
        let trailer_path = out.join("_trailer.bin");
        std::fs::write(&trailer_path, &raw[report.bytes_consumed..])?;
        println!(
            "trailer @ 0x{:08X}  size={:>9}  -> _trailer.bin",
            report.bytes_consumed,
            raw.len() - report.bytes_consumed
        );
    }

    eprintln!("extract done: {} sub-assets", total_subassets);
    Ok(())
}

fn tmd_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  notes",
        "entry", "raw", "lzs", "verts", "prims"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_verts = 0u32;
    let mut total_prims = 0u32;
    let mut entries_with_hits = 0usize;
    let mut tmds_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tmd_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tmd_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let v: u32 = scan.hits.iter().map(|(_, h)| h.total_verts).sum();
        let p: u32 = scan.hits.iter().map(|(_, h)| h.total_prims).sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, raw_hits, lzs_hits, v, p, notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
            total_verts += v;
            total_prims += p;
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>6}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tmd_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tmd_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!("{}_off{:06X}.tmd", label, hit.offset);
                std::fs::write(entry_dir.join(&fname), slab)?;
                tmds_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TMDs, {} hits total ({} verts, {} prims)",
        entries_with_hits, total_hits, total_verts, total_prims
    );
    if out.is_some() {
        println!("wrote {} TMD files", tmds_written);
    }
    Ok(())
}

fn tim_scan_cmd(
    dir: &std::path::Path,
    cdname_path: Option<&std::path::Path>,
    only_hits: bool,
    out: Option<&std::path::Path>,
) -> Result<()> {
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }

    println!(
        "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  notes",
        "entry", "raw", "lzs", "tims", "px"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut entries_with_hits = 0usize;
    let mut tims_written = 0usize;

    for path in &entries {
        let raw = std::fs::read(path)?;
        let scan = tim_scan::scan_entry(&raw);
        if scan.hits.is_empty() && only_hits {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());

        let raw_hits = scan
            .hits
            .iter()
            .filter(|(s, _)| matches!(s, tim_scan::Source::Raw))
            .count();
        let lzs_hits = scan.hits.len() - raw_hits;
        let total_px: u64 = scan
            .hits
            .iter()
            .map(|(_, h)| h.width as u64 * h.height as u64)
            .sum();
        let notes = if scan.lzs_ok { "" } else { "(lzs:no)" };
        if !scan.hits.is_empty() {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name,
                raw_hits,
                lzs_hits,
                scan.hits.len(),
                total_px,
                notes
            );
            entries_with_hits += 1;
            total_hits += scan.hits.len();
        } else if !only_hits {
            println!(
                "{:<32}  {:>4}  {:>4}  {:>5}  {:>5}  {}",
                display_name, "-", "-", "-", "-", notes
            );
        }

        if let Some(out_root) = out {
            let entry_dir = out_root.join(&display_name);
            for (src, hit) in &scan.hits {
                let (buf, label) = match src {
                    tim_scan::Source::Raw => (raw.as_slice(), "raw".to_string()),
                    tim_scan::Source::Lzs(idx) => {
                        let Some(section) = scan.lzs_sections.get(*idx) else {
                            continue;
                        };
                        (section.as_slice(), format!("lzs{}", idx))
                    }
                };
                let end = (hit.offset + hit.byte_len).min(buf.len());
                let slab = &buf[hit.offset..end];
                std::fs::create_dir_all(&entry_dir)?;
                let fname = format!(
                    "{}_off{:06X}_{}x{}_{}bpp.tim",
                    label, hit.offset, hit.width, hit.height, hit.bpp
                );
                std::fs::write(entry_dir.join(&fname), slab)?;
                tims_written += 1;
            }
        }
    }

    println!();
    println!(
        "{} entries with TIMs, {} hits total",
        entries_with_hits, total_hits
    );
    if out.is_some() {
        println!("wrote {} TIM files", tims_written);
    }
    Ok(())
}

/// `asset tim-catalog <PROT.DAT>` - flat-scan the whole archive image for
/// standard TIMs and emit the per-TIM catalog (jPSXdec parity).
fn tim_catalog_cmd(
    prot: &std::path::Path,
    out: Option<&std::path::Path>,
    rollup: bool,
) -> Result<()> {
    let catalog = tim_catalog::build_from_path(prot)?;

    if let Some(out) = out {
        let body = if out.extension().and_then(|e| e.to_str()) == Some("tsv") {
            tim_catalog::to_tsv(&catalog)
        } else {
            serde_json::to_string_pretty(&catalog)?
        };
        std::fs::write(out, body)?;
        println!("wrote {} TIMs -> {}", catalog.len(), out.display());
    } else {
        println!(
            "{:>5}  {:>10}  {:>6}  {:>14}  {:>9}  {:>4}  {:>4}  {:>9}  fnv1a",
            "id", "abs_off", "sector", "entry", "off_in", "bpp", "pal", "bytes"
        );
        println!("{}", "-".repeat(92));
        for t in &catalog {
            let entry = match t.entry_index {
                Some(i) => i.to_string(),
                None => "gap".to_string(),
            };
            println!(
                "{:>5}  0x{:08X}  {:>6}  {:>14}  0x{:07X}  {:>4}  {:>4}  {:>9}  {:016x}  {}x{}",
                t.id,
                t.abs_offset,
                t.sector,
                entry,
                t.offset_in_entry,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                t.width,
                t.height,
            );
        }
    }

    if rollup {
        let r = tim_catalog::rollup(&catalog);
        println!("rollup: count={} digest=0x{:016x}", r.count, r.digest);
    }
    Ok(())
}

/// `asset tim-deep-catalog <PROT.DAT>` - LZS-decompress every entry and
/// catalog the standard TIMs hiding inside the compressed sections.
fn tim_deep_catalog_cmd(
    prot: &std::path::Path,
    out: Option<&std::path::Path>,
    rollup: bool,
) -> Result<()> {
    let catalog = tim_deep_catalog::build_from_path(prot)?;

    if let Some(out) = out {
        let body = if out.extension().and_then(|e| e.to_str()) == Some("tsv") {
            tim_deep_catalog::to_tsv(&catalog)
        } else {
            serde_json::to_string_pretty(&catalog)?
        };
        std::fs::write(out, body)?;
        println!("wrote {} deep TIMs -> {}", catalog.len(), out.display());
    } else {
        println!(
            "{:>5}  {:>5}  {:>3}  {:>9}  {:>4}  {:>4}  {:>9}  fnv1a",
            "id", "entry", "sec", "off_in", "bpp", "pal", "bytes"
        );
        println!("{}", "-".repeat(78));
        for t in &catalog {
            println!(
                "{:>5}  {:>5}  {:>3}  0x{:07X}  {:>4}  {:>4}  {:>9}  {:016x}  {}x{}",
                t.id,
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.fnv1a,
                t.width,
                t.height,
            );
        }
    }

    if rollup {
        let r = tim_deep_catalog::rollup(&catalog);
        println!("rollup: count={} digest=0x{:016x}", r.count, r.digest);
    }
    Ok(())
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

/// Encode an RGBA8 buffer to a PNG file via the `png` crate.
fn write_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// `asset tim-render-distinct` - decode each DISTINCT cataloged TIM (deduped by
/// content fingerprint) to `<out>/<fnv>.png` and write `<out>/manifest.tsv`.
///
/// The output is decoded Sony pixel data - it is meant for local inspection
/// (driving the `tim_labels` visual categorization) and must never be
/// committed. Only the resulting fingerprint -> label table is committed.
fn tim_render_distinct_cmd(prot: &Path, out: &Path, tier: RenderTier) -> Result<()> {
    use std::collections::HashMap;

    std::fs::create_dir_all(out)?;
    let prot_bytes = std::fs::read(prot)?;

    let want_raw = matches!(tier, RenderTier::Raw | RenderTier::Both);
    let want_deep = matches!(tier, RenderTier::Deep | RenderTier::Both);

    // Per fingerprint: (tier, width, height, bpp, clut_count, count seen).
    struct Rec {
        tier: &'static str,
        w: u32,
        h: u32,
        bpp: u32,
        clut: usize,
        count: u32,
    }
    let mut recs: HashMap<u64, Rec> = HashMap::new();

    // Decode one TIM (palette 0) from a byte slice and, if its fingerprint is
    // new, write the PNG. Always bumps the per-fingerprint count.
    let mut emit = |fnv: u64,
                    bytes: &[u8],
                    tier: &'static str,
                    w: u32,
                    h: u32,
                    bpp: u32,
                    clut: usize|
     -> Result<()> {
        if let Some(r) = recs.get_mut(&fnv) {
            r.count += 1;
            return Ok(());
        }
        recs.insert(
            fnv,
            Rec {
                tier,
                w,
                h,
                bpp,
                clut,
                count: 1,
            },
        );
        if let Ok(tim) = legaia_tim::parse(bytes)
            && let Ok(rgba) = legaia_tim::decode_rgba8(&tim, 0)
        {
            let pw = tim.pixel_width() as u32;
            let ph = tim.image.h as u32;
            if pw > 0 && ph > 0 {
                write_png(&out.join(format!("{fnv:016x}.png")), &rgba, pw, ph)?;
            }
        }
        Ok(())
    };

    if want_raw {
        let archive = legaia_prot::archive::Archive::open(prot)?;
        let catalog = tim_catalog::build(&prot_bytes, &archive.entries);
        for t in &catalog {
            let off = t.abs_offset as usize;
            emit(
                t.fnv1a,
                &prot_bytes[off..off + t.byte_len],
                "raw",
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
            )?;
        }
    }

    if want_deep {
        // Decompress each entry once; decode the deep TIMs it hosts.
        let mut archive = legaia_prot::archive::Archive::open(prot)?;
        let deep = tim_deep_catalog::build(&archive, &prot_bytes);
        let entries = archive.entries.clone();
        let mut buf = Vec::new();
        // Group deep hits by entry to decompress once per entry.
        let mut by_entry: HashMap<u32, Vec<&tim_deep_catalog::DeepCatalogTim>> = HashMap::new();
        for t in &deep {
            by_entry.entry(t.entry_index).or_default().push(t);
        }
        for entry in &entries {
            let Some(hits) = by_entry.get(&entry.index) else {
                continue;
            };
            archive.read_entry(entry, &mut buf)?;
            let Ok(sections) = legaia_lzs::decompress_container(&buf) else {
                continue;
            };
            for t in hits {
                let Some(section) = sections.get(t.lzs_section as usize) else {
                    continue;
                };
                let o = t.offset_in_section as usize;
                if o + t.byte_len > section.len() {
                    continue;
                }
                emit(
                    t.fnv1a,
                    &section[o..o + t.byte_len],
                    "deep",
                    t.width,
                    t.height,
                    t.bpp,
                    t.clut_count,
                )?;
            }
        }
    }

    // Manifest, sorted by fingerprint for a stable file.
    let mut rows: Vec<(u64, &Rec)> = recs.iter().map(|(&f, r)| (f, r)).collect();
    rows.sort_by_key(|&(f, _)| f);
    let mut tsv = String::from("fnv1a\ttier\twidth\theight\tbpp\tclut_count\tcount\n");
    for (fnv, r) in &rows {
        tsv.push_str(&format!(
            "{:016x}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            fnv, r.tier, r.w, r.h, r.bpp, r.clut, r.count
        ));
    }
    std::fs::write(out.join("manifest.tsv"), tsv)?;
    println!(
        "rendered {} distinct textures -> {} (NOT for commit: decoded pixel data)",
        rows.len(),
        out.display()
    );
    Ok(())
}

/// `asset stage <PATH>` - dump one entry's stage-geometry layout. Useful
/// to confirm pool placement, sample resolved quad indices, and (with
/// `--obj-out`) export a wireframe mesh for any external viewer.
fn stage_one(input: &PathBuf, head: usize, verts: usize, obj_out: Option<&Path>) -> Result<()> {
    let raw = std::fs::read(input)?;
    let stage = stage_geom::parse(&raw)
        .ok_or_else(|| anyhow::anyhow!("no stage-geometry tables in {}", input.display()))?;
    println!(
        "file: {}  size={}  tables={}",
        input.display(),
        raw.len(),
        stage.tables.len()
    );
    for (i, t) in stage.tables.iter().enumerate() {
        println!(
            "  table[{}]: start=0x{:X} ({})  records={}  end=0x{:X}",
            i, t.start, t.start, t.records, t.end
        );
    }
    println!(
        "vertex pool: offset=0x{:X} ({})  bytes={}  verts={}",
        stage.pool_offset,
        stage.pool_offset,
        stage.pool_bytes,
        stage.vertex_count()
    );

    let largest = stage
        .tables
        .iter()
        .max_by_key(|t| t.records)
        .expect("at least one");
    println!("\nfirst {} records (resolved):", head.min(largest.records));
    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    for (i, rec) in stage_geom::records(&raw, largest).enumerate().take(head) {
        let pl = rec.payload_u16s();
        match stage.quad_vertex_indices(&rec) {
            Some(idx) => {
                let kind = if idx[3] == idx[0] { "tri" } else { "quad" };
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> {} verts {:?}",
                    i, pl[0], pl[1], pl[2], pl[3], kind, idx
                );
                resolved += 1;
            }
            None => {
                println!(
                    "  rec {:>4}: bytes [{:>5} {:>5} {:>5} {:>5}]  -> OUT OF RANGE",
                    i, pl[0], pl[1], pl[2], pl[3]
                );
                unresolved += 1;
            }
        }
    }
    // Tally for the whole table so the user knows the overall hit rate.
    let mut total_resolved = 0usize;
    for rec in stage_geom::records(&raw, largest) {
        if stage.quad_vertex_indices(&rec).is_some() {
            total_resolved += 1;
        }
    }
    println!(
        "\nresolved {}/{} records overall ({} shown above: {} ok, {} oor)",
        total_resolved,
        largest.records,
        head.min(largest.records),
        resolved,
        unresolved
    );

    println!("\nfirst {} vertices:", verts.min(stage.vertex_count()));
    for i in 0..verts.min(stage.vertex_count()) {
        let v = stage.vertex(&raw, i).expect("in range");
        println!("  v{:<4}: x={:>6} y={:>6} z={:>6}", i, v.x, v.y, v.z);
    }

    if let Some(out) = obj_out {
        write_stage_obj(&raw, &stage, largest, out)?;
        println!("\nwrote wireframe OBJ: {}", out.display());
    }
    Ok(())
}

/// Write a Wavefront OBJ with all in-range quads/tris from `table` as line
/// loops (`l` directives). Standard 3D viewers render these as wireframe.
fn write_stage_obj(
    buf: &[u8],
    stage: &stage_geom::Stage,
    table: &stage_geom::GeomTable,
    out: &Path,
) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(out)?;
    writeln!(f, "# stage-geometry wireframe")?;
    writeln!(
        f,
        "# verts={}  records={}",
        stage.vertex_count(),
        table.records
    )?;
    for i in 0..stage.vertex_count() {
        let v = stage.vertex(buf, i).unwrap();
        // OBJ is right-handed Y-up; the source is PSX Y-down, so flip Y.
        writeln!(f, "v {} {} {}", v.x, -(v.y as i32), v.z)?;
    }
    for rec in stage_geom::records(buf, table) {
        let Some(idx) = stage.quad_vertex_indices(&rec) else {
            continue;
        };
        // OBJ indices are 1-based; degenerate 4th vert (idx[3] == idx[0])
        // collapses naturally in a 4-vertex line loop.
        let a = idx[0] + 1;
        let b = idx[1] + 1;
        let c = idx[2] + 1;
        let d = idx[3] + 1;
        writeln!(f, "l {} {} {} {} {}", a, b, c, d, a)?;
    }
    Ok(())
}

/// `asset clut-finder` - walk `extracted/tim_scan/<entry>/*.tim` and report
/// every TIM whose CLUT or image rect covers the requested VRAM cell.
///
/// Used to discover which PROT entry provides a specific CLUT row that a
/// character mesh references - see `project_clut_scattering.md`.
fn clut_finder_cmd(extracted_root: &Path, x: u16, y: u16, clut_only: bool) -> Result<()> {
    let tim_scan_root = extracted_root.join("tim_scan");
    if !tim_scan_root.is_dir() {
        anyhow::bail!(
            "no tim_scan/ under {} (run `asset tim-scan` first?)",
            extracted_root.display()
        );
    }
    let mut hits: Vec<(String, String, &'static str, u16, u16, u16, u16)> = Vec::new();

    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&tim_scan_root)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();

    for sub in &subdirs {
        let entry_name = sub
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut tims: Vec<PathBuf> = std::fs::read_dir(sub)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|e| e == "tim" || e == "TIM")
                        .unwrap_or(false)
            })
            .collect();
        tims.sort();
        for tim_path in &tims {
            let bytes = match std::fs::read(tim_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let tim = match legaia_tim::parse(&bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let tim_name = tim_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            if let Some(c) = &tim.clut {
                let inside = x >= c.fb_x && x < c.fb_x + c.w && y >= c.fb_y && y < c.fb_y + c.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name.clone(),
                        "clut",
                        c.fb_x,
                        c.fb_y,
                        c.w,
                        c.h,
                    ));
                }
            }
            if !clut_only {
                let img = &tim.image;
                let inside = x >= img.fb_x
                    && x < img.fb_x + img.fb_w
                    && y >= img.fb_y
                    && y < img.fb_y + img.h;
                if inside {
                    hits.push((
                        entry_name.clone(),
                        tim_name,
                        "image",
                        img.fb_x,
                        img.fb_y,
                        img.fb_w,
                        img.h,
                    ));
                }
            }
        }
    }
    println!(
        "VRAM cell ({x}, {y}): {} match(es) across {} entries",
        hits.len(),
        subdirs.len()
    );
    println!(
        "{:<28}  {:<24}  {:<6}  {:>4} {:>4} {:>4} {:>4}",
        "entry", "tim", "kind", "fbx", "fby", "w", "h"
    );
    println!("{}", "-".repeat(80));
    for (entry, tim, kind, fx, fy, w, h) in &hits {
        println!("{entry:<28}  {tim:<24}  {kind:<6}  {fx:>4} {fy:>4} {w:>4} {h:>4}");
    }
    Ok(())
}

/// `asset stage-scan <DIR>` - scan a directory of PROT entries for
/// stage-geometry tables and report per-entry stats.
fn stage_scan_cmd(dir: &Path, cdname_path: Option<&Path>, only_hits: bool) -> Result<()> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  pool",
        "entry", "size", "tabs", "recs", "verts", "ok%"
    );
    println!("{}", "-".repeat(80));

    let mut total_hits = 0usize;
    let mut total_resolved = 0usize;
    let mut total_records = 0usize;
    for path in &paths {
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let Some(stage) = stage_geom::parse(&raw) else {
            if !only_hits {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                let display_name = display_name_for(stem, names.as_ref());
                println!(
                    "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>4}  no table",
                    display_name,
                    raw.len(),
                    "-",
                    "-",
                    "-",
                    "-"
                );
            }
            continue;
        };
        total_hits += 1;
        let largest = stage
            .tables
            .iter()
            .max_by_key(|t| t.records)
            .expect("at least one");
        let mut resolved = 0usize;
        for rec in stage_geom::records(&raw, largest) {
            if stage.quad_vertex_indices(&rec).is_some() {
                resolved += 1;
            }
        }
        total_resolved += resolved;
        total_records += largest.records;
        let pct = (100 * resolved).checked_div(largest.records).unwrap_or(0);
        let pool_side = if stage.pool_offset == 0 {
            "before"
        } else {
            "after"
        };

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let display_name = display_name_for(stem, names.as_ref());
        println!(
            "{:<32}  {:>5}  {:>4}  {:>6}  {:>6}  {:>3}%  {}",
            display_name,
            raw.len(),
            stage.tables.len(),
            largest.records,
            stage.vertex_count(),
            pct,
            pool_side
        );
    }
    println!();
    println!(
        "{} entries with stage tables; {}/{} records resolved overall ({:.1}%)",
        total_hits,
        total_resolved,
        total_records,
        if total_records > 0 {
            100.0 * total_resolved as f64 / total_records as f64
        } else {
            0.0
        }
    );
    Ok(())
}

fn field_pack_one(input: &PathBuf, all_slots: bool, groups: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(fp) = field_pack::detect(&raw) else {
        anyhow::bail!(
            "no field-pack signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = fp.preamble_range();
    let (assets_lo, assets_hi) = fp.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        fp.file_size, fp.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        fp.magic_offset,
        field_pack::MAGIC
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        fp.table_offset,
        fp.table_offset + field_pack::SCHEMA_SIZE,
        field_pack::RECORD_COUNT,
        field_pack::SCHEMA_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("schema slots:");
    let n = fp.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &fp.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    if groups {
        println!();
        println!("slot size groups (slots sharing the same size = same record kind):");
        for (size, idxs) in fp.slot_size_groups() {
            let head: Vec<String> = idxs.iter().take(10).map(|i| i.to_string()).collect();
            let tail = if idxs.len() > 10 {
                format!(" … (+{} more)", idxs.len() - 10)
            } else {
                String::new()
            };
            println!(
                "  size={:>5} (0x{:X})  count={:>3}  slots={}{}",
                size,
                size,
                idxs.len(),
                head.join(","),
                tail
            );
        }
    }
    Ok(())
}

fn field_pack_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match field_pack::detect(&raw) {
            Some(fp) => {
                hits += 1;
                let (assets_lo, assets_hi) = fp.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    fp.file_size,
                    fp.table_offset,
                    fp.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the field-pack signature",
        hits, total
    );
    Ok(())
}

fn effect_bundle_one(input: &PathBuf, all_slots: bool) -> Result<()> {
    let raw = std::fs::read(input)?;
    let Some(eb) = effect_bundle::detect(&raw) else {
        anyhow::bail!(
            "no effect-bundle signature in {} ({} bytes)",
            input.display(),
            raw.len()
        );
    };
    let (preamble_lo, preamble_hi) = eb.preamble_range();
    let (assets_lo, assets_hi) = eb.assets_range();
    println!("file:           {}", input.display());
    println!(
        "size:           {} bytes (0x{:X})",
        eb.file_size, eb.file_size
    );
    println!(
        "preamble:       0x{:X}..0x{:X} ({} bytes)",
        preamble_lo,
        preamble_hi,
        preamble_hi - preamble_lo
    );
    println!(
        "magic @         0x{:X} (= 0x{:08X})",
        eb.magic_offset,
        effect_bundle::MAGIC
    );
    println!(
        "header_a:       0x{:08X}{}",
        eb.header_a,
        if eb.header_a == effect_bundle::HEADER_A {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "header_b:       0x{:08X}{}",
        eb.header_b,
        if eb.header_b == effect_bundle::HEADER_B {
            " (= constant)"
        } else {
            " (UNEXPECTED)"
        }
    );
    println!(
        "schema table:   0x{:X}..0x{:X} ({} entries × 4 = {} bytes)",
        eb.table_offset,
        eb.table_offset + effect_bundle::TABLE_SIZE,
        effect_bundle::RECORD_COUNT,
        effect_bundle::TABLE_SIZE
    );
    println!(
        "assets region:  0x{:X}..0x{:X} ({} bytes)",
        assets_lo,
        assets_hi,
        assets_hi - assets_lo
    );
    println!();
    println!("asset region content:");
    let n_tmds = eb.assets.tmds.len();
    let n_tims = eb.assets.tims.len();
    println!(
        "  {} TMD(s) - {} master + {} sub (HEADER_A reserves 1 master + 28 sub = 29 slots)",
        n_tmds,
        n_tmds.min(1),
        n_tmds.saturating_sub(1),
    );
    if let Some(&master) = eb.assets.tmds.first() {
        println!("    master TMD @ 0x{:X} (= assets_start)", master);
    }
    if eb.assets.tmds.len() > 1 {
        let preview: Vec<String> = eb.assets.tmds[1..]
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tmds.len() > 5 {
            ", …"
        } else {
            ""
        };
        println!("    sub-TMDs   @ {}{}", preview.join(", "), suffix);
    }
    println!("  {} TIM(s)", n_tims);
    if !eb.assets.tims.is_empty() {
        let preview: Vec<String> = eb
            .assets
            .tims
            .iter()
            .take(4)
            .map(|o| format!("0x{:X}", o))
            .collect();
        let suffix = if eb.assets.tims.len() > 4 {
            ", …"
        } else {
            ""
        };
        println!("    @ {}{}", preview.join(", "), suffix);
    }
    println!();
    println!("schema slots:");
    let n = eb.slots.len();
    let show: Vec<usize> = if all_slots {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..n.min(8)).collect();
        if n > 16 {
            v.push(usize::MAX); // sentinel for ellipsis
            v.extend((n - 8)..n);
        } else {
            v.extend(8..n);
        }
        v
    };
    for i in show {
        if i == usize::MAX {
            println!("  ...");
            continue;
        }
        let s = &eb.slots[i];
        match s.size {
            Some(sz) => println!(
                "  [{:>2}] off=0x{:>5X}  size={:>5} (0x{:X})",
                i, s.offset, sz, sz
            ),
            None => println!("  [{:>2}] off=0x{:>5X}  size=  ?", i, s.offset),
        }
    }
    Ok(())
}

fn effect_bundle_scan(dir: &Path, only_hits: bool) -> Result<()> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    println!(
        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
        "entry", "size", "table_off", "preamble", "assets"
    );
    println!("{}", "-".repeat(76));
    let mut hits = 0usize;
    let mut total = 0usize;
    for path in &files {
        total += 1;
        let raw = std::fs::read(path)?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match effect_bundle::detect(&raw) {
            Some(eb) => {
                hits += 1;
                let (assets_lo, assets_hi) = eb.assets_range();
                println!(
                    "{:<32}  {:>9}  0x{:>8X}  {:>9}  {:>9}",
                    stem,
                    eb.file_size,
                    eb.table_offset,
                    eb.magic_offset,
                    assets_hi - assets_lo,
                );
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>9}  {:>10}  {:>9}  {:>9}",
                        stem,
                        raw.len(),
                        "-",
                        "-",
                        "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{} of {} entries match the effect-bundle signature",
        hits, total
    );
    Ok(())
}

fn battle_data_pack_one(input: &Path, out: Option<&Path>) -> Result<()> {
    let raw = std::fs::read(input)?;
    let pack = battle_data_pack::parse(&raw)?;
    println!("file       : {}", input.display());
    println!("file size  : {} bytes (0x{:x})", raw.len(), raw.len());
    println!(
        "table_offset: 0x{:x}, records: {}, data_base: 0x{:x}",
        pack.table_offset,
        pack.records.len(),
        pack.data_base
    );
    println!(
        "{:>3} {:>4} {:>10} {:>10} {:>10} {:>6}",
        "rec", "id", "slot_size", "data_off", "dec_size", "tmd"
    );
    let mut tmds = 0usize;
    let mut total_decoded = 0usize;
    if let Some(out) = out {
        std::fs::create_dir_all(out)?;
    }
    for r in &pack.records {
        let entry = battle_data_pack::decode_record(&raw, &pack, r.index);
        match entry {
            Ok(e) => {
                let dec_size = e.bytes.len();
                let tmd_tag = match &e.tmd_range {
                    Some(rng) => {
                        tmds += 1;
                        format!("{}..{}", rng.start, rng.end)
                    }
                    None => "-".into(),
                };
                println!(
                    "{:>3} 0x{:02x} 0x{:08x} 0x{:08x} {:>10} {:>6}",
                    r.index, r.id, r.size, r.data_offset, dec_size, tmd_tag
                );
                total_decoded += dec_size;
                if let Some(out_dir) = out {
                    let fname = format!("rec{:03}_id{:02x}.bin", r.index, r.id);
                    std::fs::write(out_dir.join(fname), &e.bytes)?;
                }
            }
            Err(err) => {
                println!(
                    "{:>3} 0x{:02x} 0x{:08x} 0x{:08x} FAIL: {}",
                    r.index, r.id, r.size, r.data_offset, err
                );
            }
        }
    }
    println!();
    println!(
        "{} records / {} bytes decompressed / {} TMDs found",
        pack.records.len(),
        total_decoded,
        tmds
    );
    Ok(())
}

fn battle_data_pack_scan(dir: &Path, cdname_path: Option<&Path>, only_hits: bool) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>7}  {:>10}  {:>5}  notes",
        "entry", "records", "dec_bytes", "tmds"
    );
    println!("{}", "-".repeat(80));
    let mut total_hits = 0usize;
    let mut total_recs = 0usize;
    let mut total_tmds = 0usize;
    for path in &entries {
        let raw = std::fs::read(path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display_name = display_name_for(&stem, names.as_ref());
        match battle_data_pack::detect(&raw) {
            Some(pack) => {
                let mut dec_bytes = 0usize;
                let mut tmds = 0usize;
                for r in &pack.records {
                    if let Ok(e) = battle_data_pack::decode_record(&raw, &pack, r.index) {
                        dec_bytes += e.bytes.len();
                        if e.tmd_range.is_some() {
                            tmds += 1;
                        }
                    }
                }
                println!(
                    "{:<32}  {:>7}  {:>10}  {:>5}",
                    display_name,
                    pack.records.len(),
                    dec_bytes,
                    tmds
                );
                total_hits += 1;
                total_recs += pack.records.len();
                total_tmds += tmds;
            }
            None => {
                if !only_hits {
                    println!("{:<32}  {:>7}  {:>10}  {:>5}", display_name, "-", "-", "-");
                }
            }
        }
    }
    println!();
    println!(
        "{} entries match, {} records, {} TMDs found",
        total_hits, total_recs, total_tmds
    );
    Ok(())
}

fn scene_v12_one(input: &Path, dump_scripts: bool, max_scripts: usize) -> Result<()> {
    let buf = std::fs::read(input)?;
    let t = legaia_asset::scene_v12_table::detect(&buf)
        .ok_or_else(|| anyhow::anyhow!("not a scene_v12_table (header magic / algebra failed)"))?;

    println!("scene_v12_table @ {}", input.display());
    println!("  size:       {} bytes", buf.len());
    println!("  N:          {} ({:#x})", t.n, t.n);
    println!("  param:      {}", t.param);
    println!(
        "  fixup slots @ +{:#x}, +{:#x}, +{:#x} (zero on disc)",
        t.table_b_base(),
        t.n,
        t.table_a_base()
    );
    println!("  end_records: {:#x}", t.end_records());
    println!();

    // Inline records at +0x14: print compact, with a small group histogram.
    println!("inline records @ +0x14 ({} entries):", t.records.len());
    let mut by_b2: std::collections::BTreeMap<u8, usize> = std::collections::BTreeMap::new();
    let head_n = t.records.len().min(24);
    for (i, r) in t.records.iter().take(head_n).enumerate() {
        println!(
            "  [{:3}] b0={:02x} b1={:02x} b2={:02x} flag={:02x}",
            i, r.b0, r.b1, r.b2, r.flag
        );
    }
    if t.records.len() > head_n {
        println!("  ... {} more not shown", t.records.len() - head_n);
    }
    for r in &t.records {
        *by_b2.entry(r.b2).or_insert(0) += 1;
    }
    print!("  b2 histogram:");
    for (b2, n) in &by_b2 {
        print!(" {:02x}×{}", b2, n);
    }
    println!();
    println!();

    // Event-script prescript at +0x800.
    println!(
        "event scripts @ +0x800: {} records, frame-opener rate {:.0}%",
        t.scripts.len(),
        100.0 * t.frame_opener_rate()
    );
    if dump_scripts {
        let show = t.scripts.len().min(max_scripts);
        for (i, r) in t.scripts.iter().take(show).enumerate() {
            let head = &buf[r.start..r.end.min(r.start + 16)];
            print!(
                "  [{:3}] @{:#06x} len={:5} {}",
                i,
                r.start,
                r.len(),
                if r.frame_opener { "OPENER" } else { "      " }
            );
            print!("  ");
            for b in head {
                print!("{:02x} ", b);
            }
            println!();
        }
        if t.scripts.len() > show {
            println!("  ... {} more not shown", t.scripts.len() - show);
        }
    }
    Ok(())
}

fn scene_v12_scan(dir: &Path, cdname_path: Option<&Path>, only_hits: bool) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    println!(
        "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>4}  notes",
        "entry", "N", "param", "b2#", "scripts", "fo%"
    );
    println!("{}", "-".repeat(80));
    let mut hits = 0usize;
    let mut total_scripts = 0usize;
    let mut high_fo = 0usize;
    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display = display_name_for(&stem, names.as_ref());
        match legaia_asset::scene_v12_table::detect(&buf) {
            Some(t) => {
                let unique_b2 = t
                    .records
                    .iter()
                    .map(|r| r.b2)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                let rate = t.frame_opener_rate();
                println!(
                    "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>3}%",
                    display,
                    t.n,
                    t.param,
                    unique_b2,
                    t.scripts.len(),
                    (rate * 100.0).round() as i32
                );
                hits += 1;
                total_scripts += t.scripts.len();
                if rate >= 0.5 {
                    high_fo += 1;
                }
            }
            None => {
                if !only_hits {
                    println!(
                        "{:<32}  {:>5}  {:>5}  {:>5}  {:>7}  {:>4}",
                        display, "-", "-", "-", "-", "-"
                    );
                }
            }
        }
    }
    println!();
    println!(
        "{hits} matches, {total_scripts} total event-script records, {high_fo} with frame-opener rate ≥ 50%"
    );
    Ok(())
}

/// LZS-decode the MAN sub-asset out of a scene_asset_table bundle entry.
///
/// Returns the decompressed MAN bytes plus the descriptor that pointed
/// at them. Bails when the buffer isn't a scene_asset_table or doesn't
/// have a type-0x03 (MAN) descriptor.
fn load_man_bytes(
    buf: &[u8],
) -> Result<(Vec<u8>, legaia_asset::scene_asset_table::DescriptorRecord)> {
    let table = legaia_asset::scene_asset_table::detect(buf)
        .ok_or_else(|| anyhow::anyhow!("not a scene_asset_table"))?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("bundle has no MAN (type 0x03) descriptor"))?;
    let start = man.data_offset as usize;
    if start >= buf.len() {
        anyhow::bail!(
            "MAN descriptor data_offset 0x{:X} past entry end ({})",
            start,
            buf.len()
        );
    }
    let (decoded, _) = legaia_lzs::decompress_tracked(&buf[start..], man.size as usize)?;
    Ok((decoded, man))
}

fn character_pack_one(
    input: &Path,
    slot: Option<usize>,
    equip: Option<u8>,
    out: Option<&Path>,
) -> Result<()> {
    use legaia_asset::character_pack;
    let bytes = std::fs::read(input)
        .with_context(|| format!("read PROT 874 entry from {}", input.display()))?;
    let pack = character_pack::parse(&bytes)?;
    let active_patches = character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;

    let print_slot = |s: &character_pack::CharacterSlot| {
        let label = character_pack::slot_label(s.slot);
        let patch = active_patches.iter().find(|p| (p.slot as usize) == s.slot);
        let patch_note = match patch {
            Some(p) => format!(
                "patched group {} @ record byte +0x{:03X}",
                p.patched_group_index, p.equip_byte_record_offset
            ),
            None => "auxiliary (no equipment swap)".to_string(),
        };
        println!(
            "  slot {} ({:<5}) disc-nobj {:2}  TMD bytes {:6}  {}",
            s.slot,
            label,
            s.disc_nobj,
            s.tmd_bytes.len(),
            patch_note,
        );
    };

    if let Some(idx) = slot {
        let slot = pack
            .slot(idx)
            .ok_or_else(|| anyhow::anyhow!("slot {idx} out of range (0..=4)"))?;
        print_slot(slot);
        if let Some(equip_byte) = equip {
            let Some(patch) = active_patches
                .iter()
                .find(|p| (p.slot as usize) == slot.slot)
            else {
                anyhow::bail!(
                    "slot {} ({}) is not an active-party slot; equipment swap only applies to 0..=2",
                    slot.slot,
                    character_pack::slot_label(slot.slot)
                );
            };
            let patched =
                character_pack::equipment_swap::apply(&slot.tmd_bytes, *patch, equip_byte);
            let template = if equip_byte == 0 { 11 } else { 10 };
            println!(
                "  applied swap: equip byte 0x{:02X} -> group-{} template overwrites visible group {}",
                equip_byte, template, patch.patched_group_index
            );
            if let Some(out_path) = out {
                std::fs::write(out_path, &patched)?;
                println!("  wrote patched TMD -> {}", out_path.display());
            }
        } else if let Some(out_path) = out {
            std::fs::write(out_path, &slot.tmd_bytes)?;
            println!("  wrote raw disc TMD -> {}", out_path.display());
        }
    } else {
        if equip.is_some() {
            anyhow::bail!("--equip requires --slot <N>");
        }
        if out.is_some() {
            anyhow::bail!("--out requires --slot <N>");
        }
        println!(
            "PROT {} (befect_data §0): {} character slots",
            character_pack::PROT_ENTRY_INDEX,
            pack.slots().len()
        );
        for s in pack.slots() {
            print_slot(s);
        }
    }
    Ok(())
}

fn field_char_tex_one(input: &Path, entry: Option<usize>, out_tim: Option<&Path>) -> Result<()> {
    use legaia_asset::field_char_textures;
    let bytes = std::fs::read(input)
        .with_context(|| format!("read PROT 874 entry from {}", input.display()))?;
    let pack = field_char_textures::parse(&bytes)?;

    println!(
        "PROT {} (player.lzs §2): {} field-texture TIM entries",
        field_char_textures::PROT_ENTRY_INDEX,
        pack.textures.len()
    );
    let role = |i: usize| match i {
        1 => "Vahn atlas page (CLUT cols 0..63)",
        2 => "Noa atlas page (CLUT cols 64..127)",
        3 => "Gala atlas page (CLUT cols 128..191)",
        6 | 7 => "atlas extension (lower)",
        _ => "shared / auxiliary page",
    };
    for t in &pack.textures {
        let img = &t.tim.image;
        let clut = t.tim.clut.as_ref();
        let (cx, cy, cn) = clut.map_or((0, 0, 0), |c| (c.fb_x, c.fb_y, c.entries.len()));
        println!(
            "  entry {} img=({:>3},{:>3}) {:>3}w x {:>3}h  clut=({:>3},{:>3}) {:>3}col  {}",
            t.index,
            img.fb_x,
            img.fb_y,
            img.fb_w,
            img.h,
            cx,
            cy,
            cn,
            role(t.index),
        );
    }

    if let Some(idx) = entry {
        let t = pack
            .textures
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("entry {idx} out of range (0..=7)"))?;
        if let Some(out_path) = out_tim {
            // Re-extract the raw TIM bytes by re-walking the pack (the parsed
            // `Tim` is lossy on exact block padding; the raw slice is exact).
            let container =
                legaia_asset::parse_player_lzs(&bytes, field_char_textures::CONTAINER_DESCRIPTORS)?;
            let section = &container.descriptors[field_char_textures::CONTAINER_SECTION];
            let decoded = legaia_asset::decode(&bytes, section, legaia_asset::DecodeMode::Lzs)?;
            let bodies = legaia_asset::pack::extract_pack(&decoded)?;
            std::fs::write(out_path, bodies[idx])?;
            println!("  wrote entry {idx} TIM -> {}", out_path.display());
        } else {
            anyhow::bail!("--entry requires --out-tim <PATH>");
        }
        let _ = t;
    } else if out_tim.is_some() {
        anyhow::bail!("--out-tim requires --entry <N>");
    }
    Ok(())
}

fn battle_char_pack_one(
    input: &Path,
    slot: Option<usize>,
    out_tmd: Option<&Path>,
    atlas: Option<usize>,
    out_tim: Option<&Path>,
) -> Result<()> {
    use legaia_asset::battle_char_pack;
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let pack = battle_char_pack::parse(&bytes)?;
    let print_slot = |s: &battle_char_pack::BattleCharSlot| {
        let label = battle_char_pack::slot_label(s.slot);
        println!(
            "  slot {} ({:<7}) disc-nobj {:2}  TMD bytes {:6}  file offset 0x{:06X}",
            s.slot,
            label,
            s.disc_nobj,
            s.tmd_bytes.len(),
            s.file_offset
        );
    };
    let print_atlas = |a: &battle_char_pack::BattleCharAtlas| {
        println!(
            "  atlas {}  CLUT fb_y={:3}  file offset 0x{:06X}  {} bytes",
            a.atlas_index,
            a.clut_fb_y,
            a.file_offset,
            a.tim_bytes.len()
        );
    };
    if let Some(s_idx) = slot {
        let s = pack
            .slot(s_idx)
            .ok_or_else(|| anyhow::anyhow!("slot {s_idx} out of range (0..=4)"))?;
        print_slot(s);
        if let Some(p) = out_tmd {
            std::fs::write(p, &s.tmd_bytes).with_context(|| format!("write {}", p.display()))?;
            println!(
                "  wrote raw disc TMD ({}) -> {}",
                battle_char_pack::slot_label(s.slot),
                p.display()
            );
        }
    } else if atlas.is_none() && out_tim.is_none() {
        println!(
            "PROT {} (other5, battle character pack): {} slots + {} atlases",
            battle_char_pack::PROT_ENTRY_INDEX,
            pack.slots().len(),
            pack.atlases.len()
        );
        for s in pack.slots() {
            print_slot(s);
        }
        for a in &pack.atlases {
            print_atlas(a);
        }
    }
    if let Some(a_idx) = atlas {
        let a = pack
            .atlas(a_idx)
            .ok_or_else(|| anyhow::anyhow!("atlas {a_idx} out of range (0..=6)"))?;
        print_atlas(a);
        if let Some(p) = out_tim {
            std::fs::write(p, &a.tim_bytes).with_context(|| format!("write {}", p.display()))?;
            println!("  wrote raw atlas {} TIM -> {}", a.atlas_index, p.display());
        }
    }
    Ok(())
}

fn player_anm_one(input: &Path, desc_count: usize, out: Option<&Path>) -> Result<()> {
    use legaia_asset::player_anm;
    let bytes = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let bundles = player_anm::find_in_entry(&bytes, desc_count);
    if bundles.is_empty() {
        println!(
            "no player-ANM bundles found in {} (desc_count={}; try 3 / 5 / 7)",
            input.display(),
            desc_count
        );
        return Ok(());
    }
    let entry_stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "entry".into());
    println!(
        "{}: {} player-ANM bundle(s)",
        input.display(),
        bundles.len()
    );
    for (i, b) in bundles.iter().enumerate() {
        let r0 = b.record_marker_1(0).unwrap_or(0);
        println!(
            "  bundle {i}: count={}  decoded={} bytes  record0 marker_1=0x{r0:04X}",
            b.record_count,
            b.decoded.len()
        );
        if let Some(out_dir) = out {
            std::fs::create_dir_all(out_dir)
                .with_context(|| format!("create_dir_all {}", out_dir.display()))?;
            let p = out_dir.join(format!("{entry_stem}_sect{i}.anm"));
            std::fs::write(&p, &b.decoded).with_context(|| format!("write {}", p.display()))?;
            println!("    wrote {} ({} bytes)", p.display(), b.decoded.len());
        }
    }
    Ok(())
}

fn player_anm_scan(dir: &Path, cdname_path: Option<&Path>, desc_count: usize) -> Result<()> {
    use legaia_asset::player_anm;
    let cdname = cdname_path
        .map(|p| std::fs::read_to_string(p).with_context(|| format!("read CDNAME {}", p.display())))
        .transpose()?;
    let cdname_map: std::collections::HashMap<u32, String> =
        cdname.as_deref().map(parse_cdname_text).unwrap_or_default();

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "BIN"))
        .collect();
    entries.sort();

    let mut total = 0usize;
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let bundles = player_anm::find_in_entry(&bytes, desc_count);
        if bundles.is_empty() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // PROT index: parse the 4-digit prefix.
        let prot_idx: u32 = name
            .split('_')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let label = cdname_map.get(&prot_idx).cloned().unwrap_or_default();
        for (i, b) in bundles.iter().enumerate() {
            total += 1;
            println!(
                "  {name:32} bundle {i}: count={:3}  decoded={:6} bytes  {}",
                b.record_count,
                b.decoded.len(),
                label
            );
        }
    }
    println!(
        "\n{total} player-ANM bundle(s) across {} entries",
        entries.len()
    );
    Ok(())
}

fn parse_cdname_text(text: &str) -> std::collections::HashMap<u32, String> {
    // CDNAME.TXT format: `#define <label> <PROT_index>` lines.
    // The label inherits forward until the next #define; we still only
    // emit the explicit (label, prot_index) pairs here.
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("#define ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(label) = parts.next() else { continue };
        let Some(idx_str) = parts.next() else {
            continue;
        };
        if let Ok(idx) = idx_str.parse::<u32>() {
            out.insert(idx, label.to_string());
        }
    }
    out
}

fn monster_archive_one(
    input: &Path,
    id: Option<u16>,
    obj: Option<&Path>,
    texture_png: Option<&Path>,
    palette: Option<usize>,
    anim: bool,
    glb: Option<&Path>,
) -> Result<()> {
    use legaia_asset::monster_archive;
    let bytes = std::fs::read(input)?;
    println!(
        "monster archive: {} bytes, {} slots of 0x{:X}",
        bytes.len(),
        monster_archive::slot_count(&bytes),
        monster_archive::SLOT_STRIDE
    );
    let print_rec = |r: &monster_archive::MonsterRecord| {
        println!(
            "  id {:3}  {:<22} HP {:5}  MP {:5}  stats {:?}  magic {}  \
             gold {:5}  exp {:5}  drop {:3}@{:3}%",
            r.id,
            r.name,
            r.hp,
            r.mp,
            r.stats,
            r.magic_count,
            r.gold,
            r.exp,
            r.drop_item,
            r.drop_chance_pct
        );
        if !r.spells.is_empty() {
            let spells: Vec<String> = r
                .spells
                .iter()
                .map(|s| {
                    let cost = if s.sp_cost == 0xFF {
                        "--".to_string()
                    } else {
                        s.sp_cost.to_string()
                    };
                    format!("0x{:02X}@{}", s.id, cost)
                })
                .collect();
            println!("        spells: {}", spells.join(" "));
        }
    };
    match id {
        Some(id) => match monster_archive::record(&bytes, id)? {
            Some(r) => print_rec(&r),
            None => println!("  id {id}: no record (out of range / filler slot)"),
        },
        None => {
            let recs = monster_archive::records(&bytes)?;
            println!("populated records: {}", recs.len());
            for r in &recs {
                print_rec(r);
            }
        }
    }
    if let Some(obj_path) = obj {
        let Some(id) = id else {
            anyhow::bail!("--obj requires --id <N>");
        };
        match monster_archive::mesh(&bytes, id)? {
            Some(m) => {
                let tmd = legaia_tmd::parse(m.tmd_bytes())?;
                let s = monster_mesh_to_obj(&tmd, m.tmd_bytes(), id);
                std::fs::write(obj_path, s)?;
                let st = tmd.stats();
                println!(
                    "  wrote mesh OBJ -> {} (TMD @ block+0x{:x}: {} verts, {} prims)",
                    obj_path.display(),
                    m.tmd_offset,
                    st.total_vertices,
                    st.total_primitives,
                );
            }
            None => println!("  id {id}: no mesh (out of range / filler / no TMD at +0x04)"),
        }
    }
    if let Some(png_path) = texture_png {
        let Some(id) = id else {
            anyhow::bail!("--texture-png requires --id <N>");
        };
        match monster_archive::mesh(&bytes, id)? {
            Some(m) => match m.texture() {
                Some(tex) => {
                    // Default to the palette the mesh's first textured prim
                    // samples (cba & 0x3F), so the page shows in real colours.
                    let pal = palette.unwrap_or_else(|| first_prim_palette(&m).unwrap_or(0));
                    let rgba = tex.to_rgba(pal);
                    write_rgba_png(png_path, tex.width as u32, tex.height as u32, &rgba)?;
                    println!(
                        "  wrote texture PNG -> {} ({}x{}, palette {}, {} palettes)",
                        png_path.display(),
                        tex.width,
                        tex.height,
                        pal,
                        tex.palettes.len(),
                    );
                }
                None => println!("  id {id}: no texture pool"),
            },
            None => println!("  id {id}: no mesh / texture (filler slot)"),
        }
    }
    if anim {
        let Some(id) = id else {
            anyhow::bail!("--anim requires --id <N>");
        };
        match monster_archive::animations(&bytes, id)? {
            Some(anims) if !anims.is_empty() => {
                println!("  action animations: {}", anims.len());
                for (i, a) in anims.iter().enumerate() {
                    // Per-part motion range over the whole animation, so the
                    // idle (index 0) is easy to eyeball vs the big move actions.
                    let (mut max_t, mut max_r) = (0i32, 0u16);
                    for f in &a.frames {
                        for p in f {
                            max_t = max_t
                                .max((p.tx as i32).abs())
                                .max((p.ty as i32).abs())
                                .max((p.tz as i32).abs());
                            max_r = max_r.max(p.rx).max(p.ry).max(p.rz);
                        }
                    }
                    println!(
                        "    [{i}] action 0x{:02X}{}  parts {:2}  frames {:3}  max|trans| {:5}  max rot {:5} (4096=turn)",
                        a.action_id,
                        if i == 0 { " (idle)" } else { "       " },
                        a.part_count,
                        a.frame_count,
                        max_t,
                        max_r,
                    );
                }
            }
            _ => println!("  id {id}: no action animations (filler slot / no mesh)"),
        }
    }
    if let Some(glb_path) = glb {
        let Some(id) = id else {
            anyhow::bail!("--glb requires --id <N>");
        };
        match legaia_asset::monster_gltf::export_glb(&bytes, id)? {
            Some(glb) => {
                std::fs::write(glb_path, &glb)?;
                println!(
                    "  wrote glTF -> {} ({} bytes, mesh + texture + animations)",
                    glb_path.display(),
                    glb.len(),
                );
            }
            None => println!("  id {id}: no exportable mesh (out of range / filler slot)"),
        }
    }
    Ok(())
}

/// The palette index (`cba & 0x3F`) of the first textured primitive in the
/// monster's mesh, so a baked texture PNG uses the colours that prim expects.
fn first_prim_palette(m: &legaia_asset::monster_archive::MonsterMesh) -> Option<usize> {
    let tbuf = m.tmd_bytes();
    let tmd = legaia_tmd::parse(tbuf).ok()?;
    let vm = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, tbuf);
    vm.cba_tsb
        .iter()
        .find(|ct| ct[0] != 0)
        .map(|ct| (ct[0] & 0x3F) as usize)
}

/// Encode an RGBA8 buffer (`width * height * 4` bytes) to a PNG file.
fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .map_err(|e| anyhow::anyhow!("write PNG: {e}"))?;
    Ok(())
}

/// Wavefront OBJ string for a monster's embedded TMD: all objects' vertices
/// concatenated, faces triangulated via the shared mesh builder.
fn monster_mesh_to_obj(tmd: &legaia_tmd::Tmd, buf: &[u8], id: u16) -> String {
    let mesh = legaia_tmd::mesh::tmd_to_mesh(tmd, buf);
    let mut s = format!(
        "# Legend of Legaia monster mesh (PROT 867 archive, id {id})\n# {} verts, {} tris\n",
        mesh.positions.len(),
        mesh.triangle_count(),
    );
    for p in &mesh.positions {
        s.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    // OBJ vertex indices are 1-based.
    for tri in mesh.indices.chunks_exact(3) {
        s.push_str(&format!("f {} {} {}\n", tri[0] + 1, tri[1] + 1, tri[2] + 1));
    }
    s
}

fn man_one(
    input: &Path,
    with_encounter: bool,
    max_formations: usize,
    max_regions: usize,
) -> Result<()> {
    let buf = std::fs::read(input)?;
    let (man_bytes, desc) = load_man_bytes(&buf)?;
    let man = legaia_asset::man_section::parse(&man_bytes)
        .map_err(|e| anyhow::anyhow!("MAN parse: {e}"))?;

    println!(
        "MAN @ {} (LZS in→out: {}→{})",
        input.display(),
        desc.size,
        man_bytes.len()
    );
    println!(
        "  status_flags    : 0x{:04X} (low_flag={}, world_map_bulk_terrain={})",
        man.header.status_flags,
        man.header.low_flag,
        man.header.world_map_bulk_terrain(),
    );
    print!("  depth_lut[16]   :");
    for v in man.header.depth_lut {
        print!(" {:>5}", v);
    }
    println!();
    println!(
        "  partitions      : N0={} N1={} N2={} (total {} records, 3-byte each)",
        man.header.partition_counts[0],
        man.header.partition_counts[1],
        man.header.partition_counts[2],
        man.header.total_records(),
    );
    println!(
        "  u24[0x28]       : 0x{:06X}  (section-0 byte offset into data region)",
        man.header.u24_at_28
    );
    println!("  data region @ 0x{:X}", man.data_region_offset);
    println!();
    println!("sections:");
    for (i, s) in man.sections.iter().enumerate() {
        let tag = match i {
            0 => " (encounter, ctrl[+0x20])",
            1 => " (ctrl[+0x00])",
            2 => " (_DAT_801C6EA0)",
            3 => " (ctrl[+0x04])",
            4 => " (DAT_80073ED8)",
            5 => " (terminator, DAT_80073EE0)",
            _ => "",
        };
        println!(
            "  [{}] @ 0x{:06X}  len=0x{:06X}  body=0x{:06X}..0x{:06X}{}",
            i,
            s.offset,
            s.length,
            s.body_offset(),
            s.end_offset(),
            tag,
        );
    }

    if with_encounter {
        println!();
        let body = man
            .encounter_section_body(&man_bytes)
            .ok_or_else(|| anyhow::anyhow!("encounter section body out of range"))?;
        let es = legaia_asset::man_section::parse_encounter_section(body)
            .map_err(|e| anyhow::anyhow!("encounter-section parse: {e}"))?;
        println!("encounter section (FUN_8003A110):");
        println!(
            "  strides: formation={} condition={} region={}",
            es.formation_stride, es.condition_stride, es.region_stride
        );
        println!(
            "  counts:  formation={} condition={} region={}  (uses {}/{} body bytes)",
            es.formation_count,
            es.condition_count,
            es.region_count,
            es.total_bytes(),
            body.len(),
        );

        let f_take = (es.formation_count as usize).min(max_formations);
        println!("  formations [{}/{}]", f_take, es.formation_count);
        for (i, f) in legaia_asset::man_section::formation_records(body, &es)
            .take(f_take)
            .enumerate()
        {
            match f {
                Some(f) => println!(
                    "    [{:3}] count={} ids=[{:>3}, {:>3}, {:>3}, {:>3}] hdr=[{:02X}, {:02X}, {:02X}] pad={}b",
                    i,
                    f.monster_count,
                    f.monster_ids[0],
                    f.monster_ids[1],
                    f.monster_ids[2],
                    f.monster_ids[3],
                    f.header_bytes[0],
                    f.header_bytes[1],
                    f.header_bytes[2],
                    f.trailing_byte_count,
                ),
                None => println!("    [{:3}] (malformed)", i),
            }
        }

        let r_take = (es.region_count as usize).min(max_regions);
        println!("  regions [{}/{}]", r_take, es.region_count);
        for (i, r) in legaia_asset::man_section::region_records(body, &es)
            .take(r_take)
            .enumerate()
        {
            match r {
                Some(r) => println!(
                    "    [{:3}] aabb=({:3},{:3})..({:3},{:3}) rate+={} formations=[{}..+{})",
                    i,
                    r.aabb_x_min,
                    r.aabb_y_min,
                    r.aabb_x_max,
                    r.aabb_y_max,
                    r.rate_increment,
                    r.formation_range_base,
                    r.formation_range_count,
                ),
                None => println!("    [{:3}] (malformed)", i),
            }
        }
    }
    Ok(())
}

fn man_scan(dir: &Path, cdname_path: Option<&Path>, json: bool) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();

    let names = match cdname_path {
        Some(p) => Some(cdname::parse(p)?),
        None => None,
    };

    #[derive(serde::Serialize)]
    struct ManScanEntry {
        entry: String,
        partition_counts: [i16; 3],
        section_lengths: [u32; 5],
        encounter_offset: usize,
        encounter_formations: Option<u8>,
        encounter_regions: Option<u8>,
        status_flags: u16,
    }

    let mut results: Vec<ManScanEntry> = Vec::new();

    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let Ok((man_bytes, _)) = load_man_bytes(&buf) else {
            continue;
        };
        let Ok(man) = legaia_asset::man_section::parse(&man_bytes) else {
            continue;
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let display = display_name_for(&stem, names.as_ref());

        let (f_n, r_n) = match man.encounter_section_body(&man_bytes) {
            Some(body) => match legaia_asset::man_section::parse_encounter_section(body) {
                Ok(es) => (Some(es.formation_count), Some(es.region_count)),
                Err(_) => (None, None),
            },
            None => (None, None),
        };

        results.push(ManScanEntry {
            entry: display,
            partition_counts: man.header.partition_counts,
            section_lengths: [
                man.sections[0].length,
                man.sections[1].length,
                man.sections[2].length,
                man.sections[3].length,
                man.sections[4].length,
            ],
            encounter_offset: man.sections[0].offset,
            encounter_formations: f_n,
            encounter_regions: r_n,
            status_flags: man.header.status_flags,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!(
            "{:<28}  {:>3} {:>3} {:>3}  enc@      s0      s1     s2     s3     s4   forms regs  flags",
            "entry", "N0", "N1", "N2"
        );
        println!("{}", "-".repeat(110));
        for r in &results {
            println!(
                "{:<28}  {:>3} {:>3} {:>3}  {:>6X}  {:>5X}  {:>5X}  {:>5X}  {:>5X}  {:>5X}  {:>4}  {:>3}  0x{:04X}",
                r.entry,
                r.partition_counts[0],
                r.partition_counts[1],
                r.partition_counts[2],
                r.encounter_offset,
                r.section_lengths[0],
                r.section_lengths[1],
                r.section_lengths[2],
                r.section_lengths[3],
                r.section_lengths[4],
                r.encounter_formations.map(|n| n as i32).unwrap_or(-1),
                r.encounter_regions.map(|n| n as i32).unwrap_or(-1),
                r.status_flags,
            );
        }
        println!();
        println!("{} scenes with a parseable MAN", results.len());
    }
    Ok(())
}

/// Build a display label for a PROT entry: `<index>_<cdname-block>` if we
/// have a name table, else just the file stem.
fn display_name_for(stem: &str, names: Option<&cdname::IndexMap>) -> String {
    if let Some(names) = names {
        // The PROT file stem looks like "0028_town0c". The numeric prefix
        // before the first underscore is the entry index.
        if let Some((num_str, _)) = stem.split_once('_')
            && let Ok(idx) = num_str.parse::<u32>()
            && let Some(block) = cdname::block_for(names, idx)
        {
            return format!("{:04}_{}", idx, block);
        }
    }
    stem.to_string()
}

/// `asset befect-cluster`: cluster-aware extraction of the `befect_data`
/// battle-effect cluster (footprint-bounded entries, LZS-container expansion,
/// per-part content classification).
fn befect_cluster_cmd(
    prot: &Path,
    cdname_path: &Path,
    out: Option<&Path>,
    json: bool,
) -> Result<()> {
    use legaia_asset::befect_cluster::{self, Component};
    use legaia_prot::archive::Archive;

    let mut archive = Archive::open(prot)?;
    let names = cdname::parse(cdname_path)?;
    let cluster = befect_cluster::extract(&mut archive, &names)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&cluster)?);
    } else {
        println!(
            "befect_data cluster: first PROT entry {}, {} parts",
            cluster.first_index,
            cluster.parts.len()
        );
        for p in &cluster.parts {
            let src = match p.lzs_section {
                Some(i) => format!("entry {} / lzs section {}", p.prot_index, i),
                None => format!("entry {}", p.prot_index),
            };
            let desc = match &p.component {
                Component::EffectScript2Pack {
                    atlas_entries,
                    anim_batches,
                    scripts,
                } => format!(
                    "efect.dat 2-pack: {atlas_entries} atlas entries, {anim_batches} anim batches, {scripts} scripts"
                ),
                Component::TmdPack { count } => format!("TMD pack: {count} effect models"),
                Component::TimImages { tims } => {
                    let mut s = format!("{} effect-texture TIM(s):", tims.len());
                    for t in tims {
                        let clut = t
                            .clut_fb
                            .map(|(x, y)| format!(" clut@({x},{y})"))
                            .unwrap_or_default();
                        s.push_str(&format!(
                            "\n        @0x{:x} {}bpp pix@fb({},{}) {}x{}hw{}",
                            t.offset, t.bpp, t.fb_x, t.fb_y, t.w_hw, t.h, clut
                        ));
                    }
                    s
                }
                Component::OffsetPack { count } => format!("offset pack: {count} entries"),
                Component::Raw => "raw / unclassified".to_string(),
            };
            println!("  [{src}] {} bytes  {desc}", p.len);
        }
    }

    if let Some(dir) = out {
        std::fs::create_dir_all(dir)?;
        for p in &cluster.parts {
            let tag = match &p.component {
                Component::EffectScript2Pack { .. } => "efect_2pack",
                Component::TmdPack { .. } => "effect_tmds",
                Component::TimImages { .. } => "effect_tims",
                Component::OffsetPack { .. } => "offset_pack",
                Component::Raw => "raw",
            };
            let name = match p.lzs_section {
                Some(i) => format!("{:04}_s{}_{}.bin", p.prot_index, i, tag),
                None => format!("{:04}_{}.bin", p.prot_index, tag),
            };
            std::fs::write(dir.join(&name), &p.data)?;
        }
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&cluster)?,
        )?;
        println!(
            "wrote {} parts + manifest.json to {}",
            cluster.parts.len(),
            dir.display()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `asset overlay` - static overlay-extraction pipeline.
// ---------------------------------------------------------------------------

use legaia_asset::static_overlay::{
    self, Eligibility, OverlayForm, OverlayRecord, ghidra_import_driver, ghidra_import_jython,
    overlay_map, recover_base, verify_fingerprint,
};

/// Read one overlay's as-loaded bytes from an already-open archive.
fn overlay_read_as_loaded(
    ar: &mut legaia_prot::archive::Archive,
    rec: &OverlayRecord,
) -> Result<Vec<u8>> {
    let entry = ar
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .with_context(|| format!("PROT entry {} not found in archive", rec.prot_index))?;
    let mut buf = Vec::new();
    ar.read_entry(&entry, &mut buf)?;
    static_overlay::as_loaded(&buf, rec)
}

fn overlay_list_cmd(json: bool) -> Result<()> {
    let map = overlay_map();
    if json {
        println!("{}", serde_json::to_string_pretty(&map.overlays)?);
        return Ok(());
    }
    println!(
        "{:>5}  {:<16}  {:<10}  {:<5}  {:<11}  clean_copy",
        "PROT", "label", "base", "form", "eligibility"
    );
    for o in &map.overlays {
        let form = match o.form {
            OverlayForm::Raw => "raw",
            OverlayForm::Lzs => "lzs",
        };
        let elig = match o.eligibility {
            Eligibility::Verified => "verified",
            Eligibility::Static => "static",
            Eligibility::Ineligible => "ineligible",
        };
        let cc = o
            .clean_copy_bytes
            .map(|n| format!("0x{n:x}"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:>5}  {:<16}  0x{:08X}  {:<5}  {:<11}  {}",
            o.prot_index, o.label, o.base_va, form, elig, cc
        );
    }
    Ok(())
}

fn overlay_extract_cmd(prot_dat: &Path, out: &Path, label: Option<&str>) -> Result<()> {
    let map = overlay_map();
    std::fs::create_dir_all(out)?;
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut wrote = 0usize;
    for rec in &map.overlays {
        if rec.eligibility == Eligibility::Ineligible {
            continue;
        }
        if label.is_some_and(|want| rec.label != want) {
            continue;
        }
        let bytes = overlay_read_as_loaded(&mut ar, rec)?;
        let path = out.join(rec.bin_filename());
        std::fs::write(&path, &bytes)?;
        println!(
            "[ok] {:<28} PROT {:>4} @ 0x{:08X}  {} bytes",
            rec.bin_filename(),
            rec.prot_index,
            rec.base_va,
            bytes.len()
        );
        wrote += 1;
    }
    println!(
        "[done] extracted {wrote} overlay blob(s) to {}",
        out.display()
    );
    Ok(())
}

fn overlay_verify_cmd(prot_dat: &Path) -> Result<()> {
    let map = overlay_map();
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut checked = 0usize;
    for rec in &map.overlays {
        if rec.fingerprint_sha256.is_none() {
            continue;
        }
        let bytes = overlay_read_as_loaded(&mut ar, rec)?;
        verify_fingerprint(rec, &bytes)?;
        println!(
            "[ok] {:<16} PROT {:>4} fingerprint reproduces ({} bytes)",
            rec.label,
            rec.prot_index,
            bytes.len()
        );
        checked += 1;
    }
    println!("[done] {checked} overlay fingerprint(s) reproduce from this disc");
    Ok(())
}

fn overlay_ghidra_cmd(out: &Path) -> Result<()> {
    let map = overlay_map();
    std::fs::create_dir_all(out)?;
    for rec in &map.overlays {
        if rec.eligibility == Eligibility::Ineligible {
            continue;
        }
        let script = ghidra_import_jython(rec);
        let path = out.join(format!("import_{}.py", rec.program_name()));
        std::fs::write(&path, script)?;
        println!("[ok] {}", path.display());
    }
    let driver = ghidra_import_driver(map);
    let driver_path = out.join("import_static_overlays.sh");
    std::fs::write(&driver_path, driver)?;
    println!("[ok] {}", driver_path.display());
    Ok(())
}

fn overlay_generate_cmd(prot_dat: &Path, indices: &[u32], min_votes: u32) -> Result<()> {
    let map = overlay_map();
    // Default to refreshing every index already in the committed map.
    let targets: Vec<u32> = if indices.is_empty() {
        map.overlays.iter().map(|o| o.prot_index).collect()
    } else {
        indices.to_vec()
    };
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    println!("# Generated by `asset overlay generate`. Review before committing.");
    for idx in targets {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => {
                eprintln!("[warn] PROT entry {idx} not found; skipping");
                continue;
            }
        };
        let mut buf = Vec::new();
        ar.read_entry(&entry, &mut buf)?;
        // Generation assumes the raw (uncompressed) as-loaded form; LZS overlays
        // must be filled in by hand with `form = "lzs"` + `decompressed_size`.
        let fp = static_overlay::fingerprint(&buf);
        let existing = map.by_prot_index(idx);
        let label = existing.map(|r| r.label.clone()).unwrap_or_default();
        let recovered = recover_base(&buf, min_votes);
        let base = recovered
            .map(|r| r.base_va)
            .or_else(|| existing.map(|r| r.base_va))
            .unwrap_or(0);
        let votes = recovered.map(|r| r.votes).unwrap_or(0);
        println!();
        println!("[[overlays]]");
        println!("prot_index = {idx}");
        println!("label = \"{label}\"");
        println!("base_va = 0x{base:08X}   # recovered votes={votes}");
        println!("form = \"raw\"");
        println!("eligibility = \"static\"");
        println!("fingerprint_sha256 = \"{fp}\"");
    }
    Ok(())
}

/// Reconnaissance sweep: for each PROT entry in `[from, to]`, recover its base
/// statically, count votes, and print the leading dev string. Not committed —
/// it's how the overlay corpus is triaged into slot-A / slot-B / non-overlay.
fn overlay_scan_cmd(
    prot_dat: &Path,
    from: u32,
    to: u32,
    min_votes: u32,
    base_filter: Option<u32>,
    json: bool,
) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Row {
        prot_index: u32,
        size: usize,
        base_va: Option<u32>,
        votes: u32,
        jal_targets: u32,
        prologues: u32,
        head: Option<String>,
    }
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut rows: Vec<Row> = Vec::new();
    let mut buf = Vec::new();
    for idx in from..=to {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => continue,
        };
        buf.clear();
        ar.read_entry(&entry, &mut buf)?;
        let rec = recover_base(&buf, min_votes);
        let base = rec.map(|r| r.base_va);
        if let Some(want) = base_filter
            && base != Some(want)
        {
            continue;
        }
        rows.push(Row {
            prot_index: idx,
            size: buf.len(),
            base_va: base,
            votes: rec.map(|r| r.votes).unwrap_or(0),
            jal_targets: rec.map(|r| r.jal_targets).unwrap_or(0),
            prologues: rec.map(|r| r.prologues).unwrap_or(0),
            head: static_overlay::head_string(&buf, 0x800, 5),
        });
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!(
        "{:>5}  {:>9}  {:<12}  {:>5}  {:>5}  {:>5}  head",
        "PROT", "size", "base", "votes", "jals", "prol"
    );
    for r in &rows {
        let base = r
            .base_va
            .map(|b| format!("0x{b:08X}"))
            .unwrap_or_else(|| "-".into());
        let head = r.head.as_deref().unwrap_or("");
        let head = if head.len() > 48 { &head[..48] } else { head };
        println!(
            "{:>5}  {:>9}  {:<12}  {:>5}  {:>5}  {:>5}  {}",
            r.prot_index, r.size, base, r.votes, r.jal_targets, r.prologues, head
        );
    }
    Ok(())
}

/// Locate a function-head signature across the corpus, printing the host PROT
/// entry + file offset (and, given the anchor VA, the implied load base). The
/// capture-free way to pin an overlay's entry — the menu-overlay method,
/// generalised into a CLI.
fn overlay_find_sig_cmd(
    prot_dat: &Path,
    sig_hex: &str,
    anchor_va: Option<u32>,
    from: u32,
    to: u32,
) -> Result<()> {
    let hex: String = sig_hex.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.len().is_multiple_of(2) {
        anyhow::bail!("signature hex must have an even number of nibbles");
    }
    let sig: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .context("parsing signature hex")?;
    if sig.is_empty() {
        anyhow::bail!("empty signature");
    }
    let mut ar = legaia_prot::archive::Archive::open(prot_dat)?;
    let mut buf = Vec::new();
    let mut hits = 0usize;
    println!(
        "# searching {} byte signature {} across PROT {from}..={to}",
        sig.len(),
        sig.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    for idx in from..=to {
        let entry = match ar.entries.iter().find(|e| e.index == idx).cloned() {
            Some(e) => e,
            None => continue,
        };
        buf.clear();
        ar.read_entry(&entry, &mut buf)?;
        if let Some(off) = static_overlay::find_signature(&buf, &sig) {
            match anchor_va {
                Some(va) => {
                    let base = va.wrapping_sub(off as u32);
                    println!("PROT {idx:>4}  file_off=0x{off:06X}  implied_base=0x{base:08X}");
                }
                None => println!("PROT {idx:>4}  file_off=0x{off:06X}"),
            }
            hits += 1;
        }
    }
    println!("# {hits} hit(s)");
    Ok(())
}
