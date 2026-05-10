//! Codified mednafen save-state capture observations.
//!
//! Each entry pins a concrete byte-level finding from a `mednafen-state diff`
//! between two save states in the `~/.mednafen/mcs/` corpus. These observations
//! are the authoritative source-of-truth for runtime memory layout that isn't
//! reachable through static analysis of `SCUS_942.54` alone - they bracket
//! what the engine knows is happening at runtime so that downstream consumers
//! (parsers, runtime hosts, integration tests) can assert against pinned
//! offsets instead of re-deriving them from raw saves.
//!
//! Conventions:
//!
//! - Every observation references the slot pair it was captured from. The
//!   matching disc-gated test in `crates/mednafen/tests/real_saves.rs`
//!   exercises the underlying save bytes against the constants below.
//! - "Pinned" means a single-byte / single-region delta has been confirmed
//!   in at least one save pair. "Inferred" means the interpretation is
//!   consistent with the data but not yet cross-validated against a static
//!   writer-search.
//! - Field offsets are quoted relative to the relevant base (character record
//!   base `0x80084708` for character-record observations, PSX virtual address
//!   for global observations).

/// One pinned byte-level RAM delta observed in a save-state diff.
#[derive(Debug, Clone)]
pub struct ByteDelta {
    /// PSX virtual address of the changed byte.
    pub addr: u32,
    /// Pre-event byte value (left side of the diff).
    pub before: u8,
    /// Post-event byte value (right side of the diff).
    pub after: u8,
}

impl ByteDelta {
    /// Compute the signed delta as an `i16` (covers the typical wraparound
    /// range without saturating at the 8-bit boundary).
    pub fn signed_delta(&self) -> i16 {
        i16::from(self.after) - i16::from(self.before)
    }
}

/// Encounter-trigger observation captured from a pre/post save pair: one
/// frame walking the `map01` field scene, then one frame with battle just
/// initiated (same `map01` scene).
///
/// Findings:
///
/// - The 133 KB MIPS / data window at `0x801CE808..0x801F3818` differs
///   wholesale between the pre-encounter and post-encounter saves - this
///   is the **battle overlay** loaded on encounter trigger. The rounded
///   extent (`0x801CE800..0x801F4000`, ~150 KB) is the canonical
///   battle-overlay residency window for the current corpus.
/// - The 8-slot battle actor pointer table populates at `0x801C9370+` with
///   stride `0x60` between adjacent slot headers (the empty-actor sentinel
///   `0x20A1 0580` flips to a per-monster pointer + control word).
/// - Scene-bundle / sound-pool writes inside `0x80083000..0x80084000`
///   surface ~600 bytes of formation + BGM resolution work. The active
///   scene index at `0x80084540` does NOT change (still `0x55` = `map01`).
///
/// Use this from engines as a hard-coded fallback for the encounter-trigger
/// transition: when crossing the boundary, the battle overlay is loaded
/// into `OVERLAY_WINDOW`, and the actor pool fills `ACTOR_POOL_WINDOW`.
pub mod encounter_trigger {
    /// Battle overlay residency window (post-trigger). The pre/post diff
    /// surfaces 133 KB of changed bytes inside this range, with no changes
    /// outside it (within the wider `0x801C0000..0x80200000` overlay
    /// region after stripping the actor-pool / scene-bundle deltas).
    pub const OVERLAY_WINDOW: (u32, u32) = (0x801CE800, 0x801F4000);

    /// 8-slot battle actor pointer table; populated post-trigger. Each
    /// slot is a `0x60`-byte header (the lower bits of `start_addr` align
    /// to the stride) carrying actor pointer + control word at offset 0.
    pub const ACTOR_POOL_WINDOW: (u32, u32) = (0x801C9370, 0x801C9900);

    /// Active scene-name table. Encounter trigger does NOT change this -
    /// the scene index stays equal to the field scene that triggered.
    pub const SCENE_NAME_TABLE_ADDR: u32 = 0x80084540;

    /// Approximate byte-count change in the overlay window between an
    /// equivalent pre-encounter / post-encounter save pair. Used for
    /// scoping assertions; tolerate ±10% drift across captures.
    pub const OVERLAY_BYTES_CHANGED_REF: usize = 133_086;

    /// Approximate byte-count change in the actor-pool window between an
    /// equivalent pre-encounter / post-encounter save pair. Captured from
    /// the wider `0x801C9300..0x801CA000` window; the narrower
    /// `ACTOR_POOL_WINDOW` captures a subset.
    pub const ACTOR_POOL_BYTES_CHANGED_REF: usize = 200;

    /// Slot stride between adjacent battle-actor pool entries.
    pub const ACTOR_POOL_SLOT_STRIDE: u32 = 0x60;

    /// Number of slots in the battle-actor pointer table.
    pub const ACTOR_POOL_SLOT_COUNT: usize = 8;
}

/// Vahn / Fire-Book-I observation captured from a pre/post save pair: one
/// frame with the battle command menu parked on Fire Book I, then one frame
/// after Fire Book I has just been used on Vahn.
///
/// **Finding (pinned).** Inside Vahn's character record (`0x80084708..+0x414`)
/// exactly one byte region differs between the two saves: a 3-byte cluster
/// at `+0x185..+0x188`.
///
/// ```text
/// pre:  +0x185 = 0x01   +0x186 = 0x0C   +0x187 = 0x00
/// post: +0x185 = 0x02   +0x186 = 0x03   +0x187 = 0x0C
/// ```
///
/// **Interpretation (inferred).** The byte at `+0x185` reads as a length
/// prefix incrementing from 1 to 2; the trailing two bytes read as list
/// entries with the new entry inserted at position 0. The byte values
/// `0x03` and `0x0C` correspond to action-constant `Attack` and direction
/// `Left` respectively in [`legaia_art::queue::ActionConstant`], which is
/// a recently-issued action history rather than a permanent learn flag.
///
/// **Caveat.** The user's reported in-game action (Fire Book I usage to
/// learn a Hyper Art) suggests the post-event state should encode a new
/// learned art. The inserted byte value `0x03` does not match any of the
/// retail learned-art constants (those occupy the `0x1B..=0x32` range -
/// see `legaia_art::tables`). Two consistent interpretations remain:
///
/// 1. The 3-byte cluster is a transient command-history buffer that the
///    item-use animation populated, unrelated to the permanent Hyper-Art
///    flag; the actual Hyper-Art learn write lives at a different offset
///    not surfaced by the pre/post diff (e.g. a global story-flag word at
///    `_DAT_1F80_0394` or a mask field outside the character record).
/// 2. The cluster is the per-character recent-action buffer the runtime
///    pre-fills before the Fire Book animation plays.
///
/// Either way, `+0x185..+0x188` is **the only** record-internal write the
/// Fire Book event produced. Engines that want to detect "Vahn just used
/// Fire Book I" can read this region and compare against the `BEFORE` /
/// `AFTER` constants below.
///
/// **Reader resolved (2026-05-10).** Grep across the captured menu
/// overlays (`overlay_menu_801d33d8.txt`, `overlay_save_ui_*`,
/// `overlay_shop_save_*` - all the same overlay, different captures)
/// found exactly one reader cluster at `0x801D4440..0x801D44A4`:
///
/// ```text
/// 801d4440  lbu t2,0x185(t2)        ; load count from char_rec[+0x185]
/// 801d4454  lbu v0,0x185(t1)
/// 801d445c  slt v0,s6,v0            ; loop while s6 < count
/// 801d4460  beq v0,zero,...
/// 801d4480  addu a0,t1,s6
/// 801d4484  lbu v0,0x0(s2)
/// 801d4498  lbu v1,0x1(s2)          ; spell-table[+1] = id
/// 801d449c  lbu v0,0x186(a0)        ; load id from char_rec[+0x186 + s6]
/// 801d44a4  beq v1,v0,...           ; match id against spell-table entry
/// ```
///
/// The structure is `[u8 count at +0x185][u8 ids[N] at +0x186..]`. The
/// menu's spell-table at `0x801E472C` is indexed by these IDs (stride
/// 0x14; `record[+0]` = sort key, `record[+1]` = ID, `record[+0xC]` =
/// name pointer). Display is capped at 7 by `slti v0,t2,0x7` later in
/// the loop, but the on-record array fits 16 bytes (the gap to the
/// equipment-slot field at +0x196), so [`MAX_DISPLAYED_SKILLS`] is 16.
///
/// The pre/post Fire Book I transition (`count: 1 → 2`, `ids[0]: 0x0C →
/// 0x03`, `ids[1]: 0x00 → 0x0C`) is a head-insert into this list - i.e.
/// the menu's displayed-skill roster grew by one new entry. Engines that
/// want a typed view should use [`legaia_save::character::CharacterRecord::displayed_skills`].
///
/// No `sb`/`sh` writers to `+0x185` exist in any captured overlay - the
/// learn writer lives in an overlay we haven't dumped (likely the item-use
/// path of the battle-action overlay, accessed via the menu rather than
/// the action SM).
///
/// [`MAX_DISPLAYED_SKILLS`]: legaia_save::character::MAX_DISPLAYED_SKILLS
pub mod vahn_fire_book_use {
    /// Vahn's character-record base in retail RAM.
    pub const VAHN_RECORD_BASE: u32 = 0x80084708;

    /// Offset of the changed cluster within Vahn's record. Aliased by
    /// [`legaia_save::character::CharacterRecord::displayed_skills`].
    pub const CHANGED_OFFSET: u32 = 0x185;

    /// Length of the changed cluster.
    pub const CHANGED_LEN: usize = 3;

    /// Pre-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
    pub const BEFORE: [u8; 3] = [0x01, 0x0C, 0x00];

    /// Post-event bytes at `VAHN_RECORD_BASE + CHANGED_OFFSET`.
    pub const AFTER: [u8; 3] = [0x02, 0x03, 0x0C];

    /// Address of the menu-overlay reader's leading instruction (`lbu
    /// t2,0x185(t2)`) - the loop that surfaces the displayed-skill list.
    pub const MENU_READER_ADDR: u32 = 0x801D4440;

    /// Address of the menu-overlay function the reader belongs to.
    /// Same address shows up across `overlay_menu_*`, `overlay_save_ui_*`,
    /// and `overlay_shop_save_*` dumps - they're identical copies of the
    /// menu overlay function.
    pub const MENU_OVERLAY_FN: u32 = 0x801D33D8;

    /// Absolute address of the cluster (handy for direct callers).
    pub const fn changed_addr() -> u32 {
        VAHN_RECORD_BASE + CHANGED_OFFSET
    }
}

/// Field-pack scene asset load observation captured from a town01 (intro
/// Rim Elm) save vs a town0c (regular Rim Elm) save. Pins the per-scene
/// runtime RAM base of field-pack scene data and the loader chain that
/// places it there.
///
/// **Loader chain (static, traced):**
///
/// 1. `FUN_801D6704` (overlay 0897, `801d6ae8`) is the scene-transition
///    asset loader. It calls `FUN_8001F7C0` with
///    `(buffer_ptr, scene_name_table=0x80084548, scene_index=0x80084540, 0)`.
///    Then it calls `FUN_80020224` (descriptor-pair walker) which
///    iterates the asset descriptor table at `_DAT_8007B85C` and
///    dispatches each entry through `FUN_8001F05C` (asset type
///    dispatcher).
/// 2. `FUN_8001F7C0` (in `SCUS_942.54`) reads the scene asset buffer
///    pointer from scratchpad cell `0x1F8003EC`, loads the field
///    asset (DATA\FIELD\\&lt;scene&gt;) into that pointer's address, then
///    loads `efect.dat` into `pointer + 0x12800`. After the load it
///    writes `pointer + 0x12800` to `_DAT_8007B8D0` for downstream
///    consumers.
/// 3. The scene transition can be initiated from a dialog-event handler
///    via `FUN_8001FD44(scene_name, sub_index)` - the dialog overlay
///    (e.g. `FUN_801D1344`) calls
///    `FUN_8001FD44("town01", 3)` when story-flag bit `0x04000000` is
///    set plus a few menu-state flags. Engines see this as a scene-name
///    write to `0x80084548` followed by an `_DAT_1F800394 |= 0x40`
///    pending-transition flip.
///
/// **Per-scene RAM base (pinned):**
///
/// - mc2 (`town01`, scene 0x03): `_DAT_8007B8D0 = 0x8014BD30`, so the
///   field-pack RAM base is `0x80139530` (= `0x8014BD30 - 0x12800`).
/// - mc0 (`town0c`, scene 0x15): `_DAT_8007B8D0 = 0x800B4DF0`, base is
///   `0x800A25F0`.
///
/// The per-scene base differs because the loader allocates from a heap
/// pool. The asset descriptor table base at `_DAT_8007B85C` is
/// **statically allocated** (= `0x8015CBD0` in both saves) and indexes
/// into the per-scene field-pack region.
///
/// **Runtime layout != on-disc schema (confirmed by capture):**
///
/// The on-disc field-pack format (see [`legaia_asset::field_pack`])
/// declares a 91 KB schema covering offsets `0x60..0x16651` with 97
/// strict slots. In RAM at `base + 0x60` (where on-disc slot 0 sits) the
/// captured bytes are post-processed GP0 GPU primitive packets, **not**
/// the schema-shaped raw NPC / event-trigger / collision records the
/// disc bytes encode. A loader transforms the on-disc preamble into a
/// runtime layout that mixes:
///
/// - GP0-shaped primitive packets (visible at `base + 0x60`)
/// - An auxiliary lookup / descriptor table elsewhere in the heap (the
///   400 KB diff window at `0x800C505C..0x80139527` between mc2 and mc0
///   is the shared scene-asset pool the loader fills before the
///   field-pack region itself)
/// - The descriptor table at `_DAT_8007B85C = 0x8015CBD0` whose entries
///   point into the field-pack region above
///
/// Pinning the **direct** preamble-byte → runtime-RAM-cell mapping
/// requires capturing the loader DURING the transition (a frame between
/// "scene change requested" and "field-pack region populated"). The
/// current single-save snapshot is post-load, so only the FINAL runtime
/// layout is observable, not the disc-byte-to-RAM-cell projection.
///
/// **Magic isn't load-bearing.** Confirmed against every captured
/// overlay and `SCUS_942.54`: no instructions compare against
/// `0x01059B84` or its split LUI/ORI immediates. The magic is a
/// build-time sanity marker.
pub mod field_pack_load {
    /// `_DAT_8007B8D0` - the loader writes `field_pack_base + 0x12800`
    /// to this cell after asset load completes. Read this value and
    /// subtract `0x12800` to recover the active per-scene base.
    pub const LOAD_DEST_PLUS_OFFSET_PTR: u32 = 0x8007B8D0;

    /// Offset added to the heap-allocated buffer pointer to compute the
    /// effect-data load destination. The loader stores
    /// `(buffer_ptr + EFFECT_OFFSET)` in [`LOAD_DEST_PLUS_OFFSET_PTR`].
    pub const EFFECT_OFFSET: u32 = 0x12800;

    /// Static asset descriptor table base. Identical across all captured
    /// saves; `FUN_80020224` walks this table after the field-pack load.
    pub const ASSET_DESCRIPTOR_TABLE_PTR_ADDR: u32 = 0x8007B85C;

    /// Pinned value of `_DAT_8007B85C` across the captured corpus.
    pub const ASSET_DESCRIPTOR_TABLE_PTR_VALUE: u32 = 0x8015CBD0;

    /// Scratchpad cell that holds the heap-resident scene asset buffer
    /// pointer. The loader reads this every transition.
    pub const SCRATCHPAD_BUFFER_PTR: u32 = 0x1F8003EC;

    /// Address of the static scene asset loader.
    pub const SCENE_ASSET_LOADER_ADDR: u32 = 0x8001F7C0;

    /// Address of the descriptor-pair walker.
    pub const DESCRIPTOR_WALKER_ADDR: u32 = 0x80020224;

    /// Address of the asset-type dispatcher.
    pub const ASSET_TYPE_DISPATCHER_ADDR: u32 = 0x8001F05C;

    /// Overlay-resident scene-transition orchestrator.
    pub const SCENE_TRANSITION_ORCHESTRATOR_ADDR: u32 = 0x801D6704;

    /// Static scene-transition setup function (writes the new scene
    /// name into the scene-name table + flips `_DAT_1F800394 |= 0x40`).
    pub const SCENE_TRANSITION_SETUP_ADDR: u32 = 0x8001FD44;

    /// Scene-transition pending bit set by `FUN_8001FD44` in
    /// `_DAT_1F800394`.
    pub const SCENE_TRANSITION_PENDING_BIT: u32 = 0x40;

    /// Field-pack RAM base for the `town01` (intro Rim Elm) save mc2.
    /// = `0x8014BD30 - 0x12800`.
    pub const TOWN01_BASE_MC2: u32 = 0x80139530;

    /// Field-pack RAM base for the `town0c` (Rim Elm Genesis Tree) save
    /// mc0. = `0x800B4DF0 - 0x12800`.
    pub const TOWN0C_BASE_MC0: u32 = 0x800A25F0;

    /// Recover the active per-scene field-pack RAM base from a save's
    /// main-RAM image. Returns `None` if the load-dest pointer reads
    /// zero (no scene loaded yet) or below `EFFECT_OFFSET`.
    pub fn recover_base(main_ram: &[u8]) -> Option<u32> {
        let off = (LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
        let bytes = main_ram.get(off..off + 4)?;
        let raw = u32::from_le_bytes(bytes.try_into().ok()?);
        if raw < EFFECT_OFFSET || !(0x80000000..0x80200000).contains(&raw) {
            return None;
        }
        Some(raw - EFFECT_OFFSET)
    }
}

/// Intra-transition observation captured from a save pair where the new
/// scene name has already been written into the scene-bundle pool but the
/// global field-pack base pointer (`_DAT_8007B8D0`) still reads the old
/// value. Pinned from the `mc2` (settled `town01` intro Rim Elm) and `mc3`
/// (`town0c` mid-transition) save pair.
///
/// Findings:
///
/// - The scene-bundle pool entry at [`SCENE_NAME_TABLE_ADDR`] flips to
///   the destination scene name (`town0c`) before the loader has populated
///   the new field-pack region. Both pool slots (`+0x08` and `+0x18`) carry
///   the new name simultaneously.
/// - `_DAT_8007B8D0` retains the previous scene's value through the
///   transition. It only flips to the new value (new field-pack base
///   `+ 0x12800`) AFTER the new region is fully populated. This pins the
///   loader's order-of-operations as: write new scene name -> populate
///   new field-pack region -> swap the global base pointer.
/// - The new field-pack region for `town0c` populates at
///   `0x800A25F0 .. 0x800B4DF0 + N` (matching the `mc0` settled values).
///   In the mid-transition snapshot, the region is partially written.
/// - The asset descriptor table at `0x8015CBD0` is bit-identical between
///   the pre- and mid-transition snapshots (4 KB SHA-256 match) - it is
///   statically allocated at boot and never relocated.
/// - The previous scene's field-pack region (here `town01` at
///   `0x80139530`) is zeroed out before the new region is populated.
///
/// This observation matters because it pins the loader projection
/// boundary: at any point during the transition, the engine can resolve
/// "which scene is residency-active" by reading the scene-bundle pool, and
/// "which scene's data is at `recover_base()`" by reading
/// [`field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR`]. The two answers can
/// disagree mid-transition, and that disagreement is the signal that the
/// loader is in flight.
pub mod field_pack_intra_transition {
    /// Scene-bundle pool base. Each pool slot is 16 bytes:
    /// `[u32 scene_id][u32 reserved][char name[8]]`. Slots 0 and 1 both
    /// carry the active / pending scene name.
    pub const SCENE_NAME_TABLE_ADDR: u32 = 0x80084540;

    /// Stride between the two scene-bundle pool slots.
    pub const SCENE_NAME_SLOT_STRIDE: u32 = 0x10;

    /// Offset of the 8-byte CDNAME label inside one scene-bundle pool slot.
    pub const SCENE_NAME_OFFSET_IN_SLOT: u32 = 0x08;

    /// Maximum length of a CDNAME label inside a pool slot (8 bytes,
    /// null-padded).
    pub const SCENE_NAME_MAX_LEN: usize = 8;

    /// Old field-pack base (`town01` intro Rim Elm, captured `mc2`).
    pub const PREV_BASE: u32 = 0x80139530;

    /// New field-pack base (`town0c` Rim Elm normal, captured `mc3`
    /// mid-transition; matches `mc0`'s settled value).
    pub const NEXT_BASE: u32 = 0x800A25F0;

    /// Read the CDNAME label from one of the two scene-bundle pool slots
    /// (`slot` is 0 or 1). Returns the trimmed label if it parses as
    /// printable ASCII, otherwise `None`.
    pub fn read_pool_slot_name(main_ram: &[u8], slot: u32) -> Option<String> {
        if slot > 1 {
            return None;
        }
        let base =
            SCENE_NAME_TABLE_ADDR + slot * SCENE_NAME_SLOT_STRIDE + SCENE_NAME_OFFSET_IN_SLOT;
        let off = (base - 0x80000000) as usize;
        let bytes = main_ram.get(off..off + SCENE_NAME_MAX_LEN)?;
        let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        if nul == 0 || !bytes[..nul].iter().all(|&b| b.is_ascii_graphic()) {
            return None;
        }
        Some(String::from_utf8_lossy(&bytes[..nul]).into_owned())
    }

    /// Detect whether `main_ram` is captured mid-transition: the
    /// scene-bundle pool's slot-0 name disagrees with the scene name
    /// implied by the field-pack base pointer's last-known value.
    /// Returns `(pool_label, recovered_base_value)` only when both
    /// readings succeed AND they disagree about which scene is loaded.
    pub fn detect_mid_transition(main_ram: &[u8]) -> Option<(String, u32)> {
        let label = read_pool_slot_name(main_ram, 0)?;
        let base = super::field_pack_load::recover_base(main_ram)?;
        // The "settled" pre-transition state (mc2 town01) has label="town01"
        // + base=PREV_BASE. The mid-transition state (mc3 town0c) has
        // label="town0c" + base=PREV_BASE - the label has flipped, the
        // base has not. We surface that case.
        if label != "town01" && base == PREV_BASE {
            return Some((label, base));
        }
        if label != "town0c" && base == NEXT_BASE {
            return Some((label, base));
        }
        None
    }
}

/// FMV cutscene overlay observation captured from `mc1` during STR
/// playback.
///
/// The cutscene overlay loads at `0x801C0000` and occupies roughly
/// `0x801CAD90..0x801F1200` (~156 KB of mixed code + data + sparse
/// zero-padding).  Key data structures pinned in the captured snapshot:
///
/// - **Compact FMV file table** at [`COMPACT_TABLE_ADDR`]: 6 entries of
///   24 bytes each, each carrying a libcd-style filename + BCD MSF + size.
///   Parsed by [`legaia_asset::str_fmv_table`].
/// - **ISO9660-shape directory copies** at [`ISO_DIRECTORY_TABLE_ADDR`]:
///   the same six files re-encoded as full ISO9660 directory records
///   (56 bytes each, includes the publisher tag `"USA"` and BE+LE LBA in
///   ISO9660 fashion). The compact table is the lookup; the directory
///   copies are presumably retained for `CdReadDir`-style validation.
/// - **STR file path strings** at [`PATH_TABLE_ADDR`]: nine null-padded
///   path strings - `\DATA\MOV.STR;1`, `\DATA\MOV15.STR;1`,
///   `\MOV\MV1A.STR;1`, plus `\MOV\MV6.STR;1` .. `\MOV\MV1.STR;1` in
///   reverse order. The reversed order matches the disc-walk order the
///   LZS / loader test scripts emit.
/// - **Mid-game FMV scene labels** at [`MID_GAME_LABELS_ADDR`]: seven
///   CDNAME-shape strings (`town0b`, `map01`, `chitei2`, `map02`, `jou`,
///   `uru2`, `town0e`). These are the field scenes that the FMV overlay
///   knows about for mid-game cutscene triggers (distinct from the `op*`
///   opening / `ed*` ending scenes that the field VM dispatches by name).
///
/// The captured slice is also exported as `/tmp/legaia_overlay_str_fmv.bin`
/// by the auto-capture pipeline and corresponds to the
/// `dump_str_fmv_overlay.py` post-script.
pub mod str_fmv_overlay {
    /// Overlay residency window (inclusive lower, exclusive upper).
    pub const OVERLAY_WINDOW: (u32, u32) = (0x801C0000, 0x80200000);

    /// Compact FMV file table. 24 bytes per entry, 6 entries.
    pub const COMPACT_TABLE_ADDR: u32 = 0x801CAE40;

    /// ISO9660-shape directory record copies. 56 bytes per entry,
    /// 6 entries. The publisher tag `"USA"` appears at +0x17 of each.
    pub const ISO_DIRECTORY_TABLE_ADDR: u32 = 0x801CCA80;

    /// Packed path string table. Nine null-padded paths covering MOV.STR,
    /// MOV15.STR, MV1A.STR, plus MV6..MV1 in reverse order.
    pub const PATH_TABLE_ADDR: u32 = 0x801CE810;

    /// Packed scene-label table for mid-game FMV-bearing field scenes.
    pub const MID_GAME_LABELS_ADDR: u32 = 0x801CE8AC;

    /// CDNAME-shape mid-game scene labels in capture order. These seven
    /// field scenes appear in the FMV overlay's data section, suggesting
    /// the FMV overlay special-cases their entry / exit transitions.
    pub const MID_GAME_LABELS: [&str; 7] = [
        "town0b", "map01", "chitei2", "map02", "jou", "uru2", "town0e",
    ];

    /// Six MV file basenames in canonical disc order (matches both the
    /// compact table and the ISO9660 directory copies).
    pub const MV_BASENAMES: [&str; 6] = [
        "MV1.STR", "MV2.STR", "MV3.STR", "MV4.STR", "MV5.STR", "MV6.STR",
    ];

    /// Detect whether the FMV overlay is residency-resident in `main_ram`.
    /// The check looks for the compact table's first entry name (`MV1.STR`)
    /// at the pinned address - if present, the overlay is loaded.
    pub fn is_resident(main_ram: &[u8]) -> bool {
        let off = (COMPACT_TABLE_ADDR - 0x80000000) as usize;
        let head = match main_ram.get(off..off + 8) {
            Some(b) => b,
            None => return false,
        };
        head.starts_with(b"MV1.STR")
    }
}

/// Character-level-up write-footprint observation pinned across the
/// `mc4..=mc9` save corpus. Three saves per character span pre /
/// during / post the level-up event:
///
/// | Character | Slot | Saves         | XP delta (u16 LE at `+0x004`) |
/// |-----------|-----:|---------------|--------------------------------|
/// | Noa       | 1    | mc4 → mc5 → mc6 → mc7 settle | `102 → 336` (+234) |
/// | Gala      | 2    | mc7 → mc8 → mc9 (settled in mc9) | `140 → 394` (+254) |
///
/// Each level-up event splits the character record write across multiple
/// frames. For Noa the captured save-pair sequence pins:
///
/// 1. **Record write (`mc4 → mc5`)**. The persistent record stat window
///    at `+0x11C..+0x12D` (9 u16 LE values), the XP at `+0x004..+0x005`,
///    and the rank counter at `+0x130` are written in one frame. The
///    live in-battle stat copy at `+0x104..+0x11B` is unchanged at this
///    point.
/// 2. **Live copy (`mc5 → mc6`)**. The live stat copy mirrors the record
///    copy: HP_cur (`+0x104`), MP_cur (`+0x108`), and the six u16 stats at
///    `+0x110..+0x11B` settle to their post-level-up values. HP_max /
///    MP_max / SP_max in the live copy at `+0x106 / +0x10A / +0x10E`
///    have NOT yet been written.
/// 3. **Settle (`mc6 → mc7`)**. The live HP_max / MP_max / SP_max settle
///    at `+0x106 / +0x10A / +0x10E`. After this frame the live and
///    record copies of HP_max / MP_max agree.
///
/// Gala's level-up sequence runs in two phases (mc7 → mc8 record, mc8 →
/// mc9 live + settle); the Gala capture lacks a dedicated "record-only"
/// frame, so HP_max / MP_max / SP_max in the live copy land in the
/// `mc8 → mc9` step alongside HP_cur / MP_cur / live stats.
///
/// The pinned byte deltas live in [`crate::levelup::observations`]:
/// - [`crate::levelup::observations::noa_mc4_to_mc7`]
/// - [`crate::levelup::observations::gala_mc7_to_mc9`]
///
/// **Per-character record bases** (verified across the captured corpus,
/// stride `0x414`):
/// - Vahn: `0x80084708`
/// - Noa:  `0x80084B1C`
/// - Gala: `0x80084F30`
/// - Slot 3: `0x80085344`
///
/// **Record stat window** at `+0x11C..+0x12D` (9 u16 LE values). Values
/// pinned across the corpus:
/// - `[+0x11C]` HP_max — mirrors live `+0x106`.
/// - `[+0x11E]` MP_max — mirrors live `+0x10A`.
/// - `[+0x120]` per-stat cap constant `100` — unchanged across every
///   captured save and every character.
/// - `[+0x122..+0x12D]` six u16 record-side stats — mirror live
///   `+0x110..+0x11B`.
///
/// SP_max at `+0x10E` lives only in the live in-battle copy. The record
/// at `+0x120` is a 100-cap constant, **not** SP_max. Noa's level-up
/// grants `+40` SP_max (Seru-magic user); Gala's grants `0` (physical
/// Tactical Arts user).
pub mod char_level_up {
    /// PSX-virtual-address base of the character record table.
    pub const TABLE_BASE: u32 = 0x80084708;

    /// Per-character record stride.
    pub const RECORD_STRIDE: u32 = 0x414;

    /// Vahn's character record base address.
    pub const VAHN_BASE: u32 = TABLE_BASE;
    /// Noa's character record base address (slot 1).
    pub const NOA_BASE: u32 = TABLE_BASE + RECORD_STRIDE;
    /// Gala's character record base address (slot 2).
    pub const GALA_BASE: u32 = TABLE_BASE + 2 * RECORD_STRIDE;
    /// Fourth party slot record base address.
    pub const SLOT3_BASE: u32 = TABLE_BASE + 3 * RECORD_STRIDE;

    /// Offset within the record where the level-up event writes the live
    /// in-battle stat copy: HP_cur, HP_max, MP_cur, MP_max, SP_cur,
    /// SP_max (six u16s) at `+0x104..+0x110`, then six u16 live stats at
    /// `+0x110..+0x11C`.
    pub const LIVE_WINDOW: (u32, u32) = (0x104, 0x11C);

    /// Offset within the record of the persistent stat window (9 u16 LE
    /// values: HP_max, MP_max, cap, six stats).
    pub const RECORD_WINDOW: (u32, u32) = (0x11C, 0x12E);

    /// Offset of the rank counter (single byte, increments by 1 per
    /// level-up event).
    pub const RANK_COUNTER: u32 = 0x130;

    /// Offset of the XP low word (u16 LE).
    pub const XP_LO: u32 = 0x004;

    /// Per-stat cap constant value. Unchanged across every captured
    /// save; the `+0x120` u16 LE field carries this exact value for
    /// Vahn, Noa, and Gala in every state.
    pub const RECORD_STAT_CAP: u16 = 100;

    /// Read a character's record-window u16 LE deltas across two saves.
    /// Returns the 9 u16 values for the given record base in `main_ram`.
    pub fn read_record_stats(main_ram: &[u8], record_base: u32) -> Option<[u16; 9]> {
        let off = (record_base - 0x80000000) as usize + RECORD_WINDOW.0 as usize;
        let end = off + 18;
        let bytes = main_ram.get(off..end)?;
        let mut out = [0u16; 9];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
        }
        Some(out)
    }

    /// Read the rank counter for the given record base.
    pub fn read_rank_counter(main_ram: &[u8], record_base: u32) -> Option<u8> {
        let off = (record_base - 0x80000000) as usize + RANK_COUNTER as usize;
        main_ram.get(off).copied()
    }

    /// Read the cumulative XP (u16 LE) at `+0x004`.
    pub fn read_xp_u16(main_ram: &[u8], record_base: u32) -> Option<u16> {
        let off = (record_base - 0x80000000) as usize + XP_LO as usize;
        let bytes = main_ram.get(off..off + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_delta_signed_delta_arithmetic() {
        let d = ByteDelta {
            addr: 0x80084708 + 0x10E,
            before: 0x3A,
            after: 0x42,
        };
        assert_eq!(d.signed_delta(), 8);

        let neg = ByteDelta {
            addr: 0x80084708 + 0x11C,
            before: 0xDD,
            after: 0x03,
        };
        // 0x03 - 0xDD = -218 (the actual u16 LE field underneath wraps,
        // but the byte-only signed delta is what we surface).
        assert_eq!(neg.signed_delta(), -218);
    }

    #[test]
    fn encounter_trigger_overlay_window_covers_documented_range() {
        let (lo, hi) = encounter_trigger::OVERLAY_WINDOW;
        assert!(lo < hi);
        assert!(lo <= 0x801CE808);
        assert!(hi >= 0x801F3818);
        // Sanity: window spans roughly the documented 150 KB.
        assert!((hi - lo) as usize >= 0x20_000);
        assert!((hi - lo) as usize <= 0x40_000);
    }

    #[test]
    fn encounter_trigger_actor_pool_stride_is_consistent() {
        let (lo, hi) = encounter_trigger::ACTOR_POOL_WINDOW;
        let span = hi - lo;
        let n = encounter_trigger::ACTOR_POOL_SLOT_COUNT as u32;
        let stride = encounter_trigger::ACTOR_POOL_SLOT_STRIDE;
        assert!(span >= n * stride);
    }

    #[test]
    fn vahn_fire_book_changed_addr_is_inside_record() {
        let addr = vahn_fire_book_use::changed_addr();
        assert!(addr >= vahn_fire_book_use::VAHN_RECORD_BASE);
        assert!(addr < vahn_fire_book_use::VAHN_RECORD_BASE + 0x414);
    }

    #[test]
    fn char_level_up_record_bases_are_stride_consistent() {
        assert_eq!(char_level_up::VAHN_BASE, 0x80084708);
        assert_eq!(char_level_up::NOA_BASE, char_level_up::VAHN_BASE + 0x414);
        assert_eq!(
            char_level_up::GALA_BASE,
            char_level_up::VAHN_BASE + 2 * 0x414
        );
        assert_eq!(
            char_level_up::SLOT3_BASE,
            char_level_up::VAHN_BASE + 3 * 0x414
        );
    }

    #[test]
    fn char_level_up_record_window_spans_18_bytes() {
        let (lo, hi) = char_level_up::RECORD_WINDOW;
        assert_eq!(hi - lo, 18);
    }

    #[test]
    fn char_level_up_readers_lift_from_synthesised_main_ram() {
        let mut ram = vec![0u8; 0x200000];
        let off = (char_level_up::NOA_BASE - 0x80000000) as usize;
        // Plant XP = 336 at +0x004.
        ram[off + 0x004] = 0x50;
        ram[off + 0x005] = 0x01;
        // Plant a record stat window: HP_max = 182, MP_max = 16, cap = 100,
        // six stats = 124, 24, 16, 13, 34, 6.
        let stats: [u16; 9] = [182, 16, 100, 124, 24, 16, 13, 34, 6];
        for (i, s) in stats.iter().enumerate() {
            let lo = (*s & 0xFF) as u8;
            let hi = (*s >> 8) as u8;
            ram[off + 0x11C + i * 2] = lo;
            ram[off + 0x11C + i * 2 + 1] = hi;
        }
        // Plant rank = 2.
        ram[off + 0x130] = 2;

        assert_eq!(
            char_level_up::read_xp_u16(&ram, char_level_up::NOA_BASE),
            Some(336)
        );
        assert_eq!(
            char_level_up::read_rank_counter(&ram, char_level_up::NOA_BASE),
            Some(2)
        );
        let lifted = char_level_up::read_record_stats(&ram, char_level_up::NOA_BASE).unwrap();
        assert_eq!(lifted, stats);
        assert_eq!(lifted[2], char_level_up::RECORD_STAT_CAP);
    }

    #[test]
    fn field_pack_recover_base_handles_zero_and_below_offset() {
        // Empty RAM: load-dest pointer is zero, recovery should fail.
        let zero = vec![0u8; 0x100000];
        assert!(field_pack_load::recover_base(&zero).is_none());

        // Plant the pinned mc2 value (`0x8014BD30`) at the right offset.
        let mut ram = vec![0u8; 0x100000];
        let off = (field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
        ram[off..off + 4].copy_from_slice(&0x8014BD30u32.to_le_bytes());
        let base = field_pack_load::recover_base(&ram).expect("should recover");
        assert_eq!(base, field_pack_load::TOWN01_BASE_MC2);
    }

    #[test]
    fn field_pack_constants_round_trip_through_recover() {
        assert_eq!(
            field_pack_load::TOWN01_BASE_MC2 + field_pack_load::EFFECT_OFFSET,
            0x8014BD30
        );
        assert_eq!(
            field_pack_load::TOWN0C_BASE_MC0 + field_pack_load::EFFECT_OFFSET,
            0x800B4DF0
        );
    }

    #[test]
    fn intra_transition_pool_slot_name_round_trips() {
        // Build a synthetic main-RAM image with "town0c" planted in slot 0.
        let mut ram = vec![0u8; 0x100000];
        let off = (field_pack_intra_transition::SCENE_NAME_TABLE_ADDR
            + field_pack_intra_transition::SCENE_NAME_OFFSET_IN_SLOT
            - 0x80000000) as usize;
        ram[off..off + 6].copy_from_slice(b"town0c");
        let label = field_pack_intra_transition::read_pool_slot_name(&ram, 0);
        assert_eq!(label.as_deref(), Some("town0c"));
        // Slot 1 is empty (no name) - reading should fail gracefully.
        assert!(field_pack_intra_transition::read_pool_slot_name(&ram, 1).is_none());
        // Slot 2 doesn't exist.
        assert!(field_pack_intra_transition::read_pool_slot_name(&ram, 2).is_none());
    }

    #[test]
    fn intra_transition_detector_flags_label_base_disagreement() {
        // Plant the mc3 mid-transition shape: slot 0 says "town0c",
        // _DAT_8007B8D0 still says PREV_BASE+0x12800 (the old town01 base).
        let mut ram = vec![0u8; 0x200000];
        let pool_off = (field_pack_intra_transition::SCENE_NAME_TABLE_ADDR
            + field_pack_intra_transition::SCENE_NAME_OFFSET_IN_SLOT
            - 0x80000000) as usize;
        ram[pool_off..pool_off + 6].copy_from_slice(b"town0c");
        let load_dest_off = (field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
        let stale_load_dest =
            field_pack_intra_transition::PREV_BASE + field_pack_load::EFFECT_OFFSET;
        ram[load_dest_off..load_dest_off + 4].copy_from_slice(&stale_load_dest.to_le_bytes());

        let mid = field_pack_intra_transition::detect_mid_transition(&ram);
        assert_eq!(
            mid,
            Some(("town0c".to_string(), field_pack_intra_transition::PREV_BASE))
        );

        // Settled state (mc2): slot 0 says "town01" + base = PREV_BASE.
        // detector should NOT flag this case.
        ram[pool_off..pool_off + 6].copy_from_slice(b"town01");
        assert!(field_pack_intra_transition::detect_mid_transition(&ram).is_none());
    }

    #[test]
    fn fmv_overlay_resident_check_passes_on_planted_signature() {
        // FMV overlay residency is detected by the "MV1.STR" prefix at the
        // pinned compact-table address.
        let mut ram = vec![0u8; 0x200000];
        assert!(!str_fmv_overlay::is_resident(&ram));
        let off = (str_fmv_overlay::COMPACT_TABLE_ADDR - 0x80000000) as usize;
        ram[off..off + 9].copy_from_slice(b"MV1.STR;1");
        assert!(str_fmv_overlay::is_resident(&ram));
    }

    #[test]
    fn fmv_overlay_mid_game_labels_are_lowercase_cdname_shape() {
        for label in str_fmv_overlay::MID_GAME_LABELS {
            assert!(!label.is_empty());
            assert!(label.len() <= 8, "{label} exceeds CDNAME slot width");
            assert!(
                label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{label} not CDNAME-shape"
            );
        }
    }

    #[test]
    fn fmv_overlay_mv_basenames_are_canonical_order() {
        let last_digit = |s: &str| s.chars().nth(2).unwrap().to_digit(10).unwrap();
        for (i, name) in str_fmv_overlay::MV_BASENAMES.iter().enumerate() {
            assert_eq!(last_digit(name), (i as u32) + 1);
        }
    }

    #[test]
    fn vahn_fire_book_pattern_matches_pinned_capture() {
        // Pre-event has count=1, list=[0x0C], slot[1]=0x00.
        assert_eq!(vahn_fire_book_use::BEFORE, [0x01, 0x0C, 0x00]);
        // Post-event has count=2, list=[0x03, 0x0C].
        assert_eq!(vahn_fire_book_use::AFTER, [0x02, 0x03, 0x0C]);
        // Count byte incremented by 1 (regardless of interpretation).
        assert_eq!(
            vahn_fire_book_use::AFTER[0] - vahn_fire_book_use::BEFORE[0],
            1
        );
        // Pre-event entry at position 0 (`0x0C`) appears at position 1
        // post-event - consistent with insertion at the front.
        assert_eq!(vahn_fire_book_use::AFTER[2], vahn_fire_book_use::BEFORE[1]);
    }
}
