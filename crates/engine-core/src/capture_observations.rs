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
/// - `town01` (scene 0x03): `_DAT_8007B8D0 = 0x8014BD30`, so the
///   field-pack RAM base is `0x80139530` (= `0x8014BD30 - 0x12800`).
/// - `town0c` (scene 0x15): `_DAT_8007B8D0 = 0x800B4DF0`, base is
///   `0x800A25F0`.
///
/// The per-scene base differs because the loader allocates from a heap
/// pool. The asset descriptor table base at `_DAT_8007B85C` is
/// **statically allocated** (= `0x8015CBD0` in every captured save)
/// and indexes into the per-scene field-pack region.
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
///   400 KB diff window at `0x800C505C..0x80139527` between the
///   `town01` and `town0c` saves is the shared scene-asset pool the
///   loader fills before the field-pack region itself)
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

    /// Field-pack RAM base for the `town01` (intro Rim Elm) save.
    /// = `0x8014BD30 - 0x12800`.
    pub const TOWN01_FIELD_PACK_BASE: u32 = 0x80139530;

    /// Field-pack RAM base for the `town0c` (Rim Elm Genesis Tree)
    /// save. = `0x800B4DF0 - 0x12800`.
    pub const TOWN0C_FIELD_PACK_BASE: u32 = 0x800A25F0;

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
/// value. Pinned from a settled `town01` intro Rim Elm save paired
/// with a mid-transition `town0c` save.
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
///   `0x800A25F0 .. 0x800B4DF0 + N` (matching the settled-state
///   values from a fully-loaded `town0c` save). In the
///   mid-transition snapshot, the region is partially written.
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

    /// Old field-pack base (`town01` intro Rim Elm settled state).
    pub const PREV_BASE: u32 = 0x80139530;

    /// New field-pack base (`town0c` Rim Elm normal scene; matches
    /// the settled `town0c` value once the loader completes).
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
        // The settled pre-transition state (`town01`) has
        // label="town01" + base=PREV_BASE. The mid-transition state
        // (`town0c`) has label="town0c" + base=PREV_BASE — the label
        // has flipped, the base has not. We surface that case.
        if label != "town01" && base == PREV_BASE {
            return Some((label, base));
        }
        if label != "town0c" && base == NEXT_BASE {
            return Some((label, base));
        }
        None
    }
}

/// FMV cutscene overlay observation captured from a save state taken
/// during STR playback (FMV-overlay-resident state).
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

/// Character-level-up write-footprint observation pinned across a
/// per-character pre / mid / post save triplet (battle scene `map01`,
/// 4-level XP jump):
///
/// | Character | Slot | Triplet shape       | XP delta (u16 LE at `+0x004`) |
/// |-----------|-----:|---------------------|--------------------------------|
/// | Noa       | 1    | pre → record → live → settle | `102 → 336` (+234) |
/// | Gala      | 2    | pre → record → live+settle | `140 → 394` (+254) |
///
/// Each level-up event splits the character record write across
/// multiple frames. For Noa the captured triplet pins three phases:
///
/// 1. **Record write**. The persistent record stat window at
///    `+0x11C..+0x12D` (9 u16 LE values), the XP at `+0x004..+0x005`,
///    and the rank counter at `+0x130` are written in one frame. The
///    live in-battle stat copy at `+0x104..+0x11B` is unchanged at this
///    point.
/// 2. **Live copy**. The live stat copy mirrors the record copy: HP_cur
///    (`+0x104`), MP_cur (`+0x108`), and the six u16 stats at
///    `+0x110..+0x11B` settle to their post-level-up values. HP_max /
///    MP_max / SP_max in the live copy at `+0x106 / +0x10A / +0x10E`
///    have NOT yet been written.
/// 3. **Settle**. The live HP_max / MP_max / SP_max settle at
///    `+0x106 / +0x10A / +0x10E`. After this frame the live and record
///    copies of HP_max / MP_max agree.
///
/// Gala's level-up sequence runs in two phases (record → live+settle in
/// one frame); the Gala capture lacks a dedicated "record-only" frame,
/// so HP_max / MP_max / SP_max in the live copy land in the live-copy
/// step alongside HP_cur / MP_cur / live stats.
///
/// The pinned byte deltas live in [`crate::levelup::observations`]:
/// - [`crate::levelup::observations::noa_4_level_jump`]
/// - [`crate::levelup::observations::gala_4_level_jump`]
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

/// Battle scene-init transition observation captured from a save pair:
/// one save in the active field scene with the encounter armed but
/// battle not yet entered, then one save with battle just initiated
/// against the same encounter.
///
/// The captured pair is a `map01` battle scene with the formation cell
/// at `0x8007BD0C` flipped from `00 00 00 00` (no formation) to
/// `04 04 00 00` (count = 2, two copies of monster id `0x04`). The
/// scene-bundle pool at `0x80084540` carries `map01` in both saves -
/// the pre-battle save is on the field-pack-bearing side of the same
/// scene rather than the world-map (battles layer over the active
/// field scene rather than swapping it out).
///
/// Findings:
///
/// - The 168 KB region at `0x80124690..0x801503C4` flips from a
///   field-scene payload (sample dialog text visible, e.g. the
///   "Hold still, I am going to lick your wounds." string from the
///   intro field) to battle-bundle data (vertex / TIM / actor records).
///   This is the **battle-data init load window** - whatever overlay
///   handler kicks the battle scene-init copies the battle bundle
///   here.
/// - The 16 KB region at `0x801CE808..0x801D3018` flips wholesale -
///   this is the **battle-overlay scratch slice** the battle action
///   handlers reset on entry. Distinct from the broader battle-action
///   overlay code/data residency (which extends further up to
///   `0x801F4000`).
/// - The 8-slot battle actor pointer table at `0x801C9370+` (stride
///   `0x60`) populates with 5 active slots (slots 0..2 = party,
///   slots 3..4 = the two monsters, slots 5..7 cleared). Confirmed
///   against the count-2 formation: only 2 monster slots filled.
/// - The CD I/O state slice at `0x801FFCA0..0x801FFFFE` rewires
///   pending sector reads as the battle bundle is paged in - a
///   reliable "battle scene-init in flight" signature.
/// - The bundle-pool extension at `0x80083680..0x80083820` carries
///   ~80 bytes of battle-side function-pointer wiring; one slot at
///   `0x800836F8` flips to `0xF41D0280` (= `FUN_80021DF4`, the
///   per-frame actor tick), making the bundle-pool extension a
///   reliable "battle ticker armed" signal.
///
/// The single remaining gap is the **stat-grant table LOADER**: a
/// static helper (still in an uncaptured overlay slice) that reads
/// the per-Seru stat-grant data off PROT entry `0x05C4` + sibling
/// Seru blobs.
///
/// This save pair pins the **residency window** the loader writes
/// into; the loader itself is not directly visible in either
/// snapshot (it has finished by the time the post-load save is
/// captured).
pub mod battle_init_overlay {
    use super::ByteDelta;

    /// 168 KB battle-bundle residency window: the field-scene payload
    /// is overwritten here when battle scene-init runs. Computed from
    /// the captured `mednafen-state diff` extent.
    pub const BATTLE_BUNDLE_WINDOW: (u32, u32) = (0x80124690, 0x801503C4);

    /// 16 KB battle-overlay scratch slice. Resets on battle entry;
    /// distinct from the broader battle-action overlay residency at
    /// `0x801CE800..0x801F4000`.
    pub const OVERLAY_SCRATCH_WINDOW: (u32, u32) = (0x801CE808, 0x801D3018);

    /// 8-slot battle actor pointer table; populated post-trigger.
    pub const ACTOR_POOL_BASE: u32 = 0x801C9370;
    /// Stride between adjacent actor-pointer slots (header bytes).
    pub const ACTOR_POOL_SLOT_STRIDE: u32 = 0x60;
    /// 8 slots: 0..2 party, 3..7 monsters (per the existing battle
    /// pointer-table doc).
    pub const ACTOR_POOL_SLOT_COUNT: u32 = 8;

    /// Bundle-pool extension that picks up the per-frame actor tick
    /// pointer when battle scene-init completes.
    pub const BUNDLE_POOL_EXTENSION_BASE: u32 = 0x80083680;
    /// Address inside the extension where the per-frame actor tick
    /// pointer (`FUN_80021DF4 = 0x80021DF4`) lands once battle is up.
    /// The slot holds a non-battle handler (`0x80024C50`) before
    /// scene-init runs.
    pub const ACTOR_TICK_FN_PTR_ADDR: u32 = 0x800836C8;
    /// Expected value once battle scene-init completes.
    pub const ACTOR_TICK_FN_PTR_VALUE: u32 = 0x80021DF4;

    /// CD I/O state slice that re-wires while the battle bundle is
    /// paged in. A non-zero diff over this window plus a stable
    /// scene-name table is a reliable "battle scene-init in flight"
    /// signature.
    pub const CD_IO_STATE_WINDOW: (u32, u32) = (0x801FFCA0, 0x801FFFFE);

    /// Formation cell address. Pre/post deltas: `00 00 00 00` →
    /// `04 04 00 00` for the captured pair.
    pub const FORMATION_CELL_ADDR: u32 = 0x8007BD0C;

    /// Encounter delta against the captured pair (count=0 → count=2,
    /// monster id 4 in slots 0..1). Independent of which encounter
    /// the user captured - if a different formation is captured this
    /// constant becomes documentation rather than an assertion.
    pub const FORMATION_CELL_DELTAS: [ByteDelta; 2] = [
        ByteDelta {
            addr: FORMATION_CELL_ADDR,
            before: 0x00,
            after: 0x04,
        },
        ByteDelta {
            addr: FORMATION_CELL_ADDR + 1,
            before: 0x00,
            after: 0x04,
        },
    ];
}

/// Battle action animation observation captured from a save pair:
/// one save with the action menu armed but no animation in flight,
/// and one save mid-action-animation (somersault / strike pose).
///
/// The captured pair shares the active scene `map01` and battle
/// state. The actor record stride is `0x2D4` bytes; all observations
/// here are offsets relative to a single actor record base
/// (slot 0 base = `0x800EC9E8`).
///
/// Findings (unblocks PRD §2.1, the ANM opaque-record interpreter):
///
/// - The **per-actor anim-PC** lives at `+0x1D8..+0x1E8` (16 bytes).
///   In the pre-animation save the window is mostly zero with a
///   sentinel pair `01 77` at offset `+0x1D7..+0x1D8`; in the
///   mid-animation save the window holds incrementing per-bone
///   counters (e.g. `00 11 00 27 00 03 03 0F 0E 19 27 00`).
/// - The **per-frame anim flag accumulator** lives at `+0x1F4..+0x205`
///   (18 bytes). Pre-animation values are all zero; mid-animation
///   the bytes monotonically transition to a stamped run of `0x11`
///   bytes once the action engages.
/// - A 4-pointer **animation dispatch table** lives at `+0x234..+0x244`
///   (16 bytes; 4 × u32). Each pointer holds the same value (the
///   active anim record). Pre-animation = `0x8015CC30`;
///   mid-animation = `0x801621D0`. The pointer is bumped between
///   the two captures by `+0x55A0` bytes - the loader has paged a
///   different ANM record into a different position in the heap.
/// - The anim-record header at the dispatch pointer (read from
///   `0x801621D0`) shows the shape `[u32 len=0x18, _, _, u32 4,
///   u32 5, u32 0x00299307, u32 0x0140017E]` - consistent with a
///   small (24 byte) per-record control block carrying the kind
///   word, frame count, dispatch flags, and the first opcode block.
///
/// These offsets bracket the per-actor anim state to a small named
/// region; combined with a Ghidra dump of the per-record dispatch
/// jump table (whose entry point is the value written into the
/// dispatch pointer above) the per-kind opcodes can be lifted.
pub mod battle_action_animation {
    /// Actor record stride.
    pub const ACTOR_RECORD_STRIDE: u32 = 0x2D4;

    /// Slot 0 (party leader) actor record base. The 8-slot pool
    /// continues at `+ ACTOR_RECORD_STRIDE` for each subsequent slot.
    pub const SLOT0_ACTOR_RECORD_BASE: u32 = 0x800EC9E8;

    /// Per-actor anim-PC region (16 bytes). Holds increment-style
    /// per-bone or per-frame cursors.
    pub const ANIM_PC_FIELD_OFFSET: u32 = 0x1D8;
    /// Length of the anim-PC region.
    pub const ANIM_PC_FIELD_LEN: u32 = 0x10;

    /// Per-frame anim flag accumulator (18 bytes).
    pub const ANIM_FRAME_FLAGS_OFFSET: u32 = 0x1F4;
    /// Length of the flag accumulator.
    pub const ANIM_FRAME_FLAGS_LEN: u32 = 0x12;

    /// 4 × u32 anim dispatch pointer table.
    pub const ANIM_DISPATCH_PTR_TABLE_OFFSET: u32 = 0x234;
    /// 4 × u32 = 16 bytes.
    pub const ANIM_DISPATCH_PTR_TABLE_LEN: usize = 16;

    /// Resolve the absolute address of the dispatch-pointer slot 0
    /// for a given actor record base.
    pub fn dispatch_ptr_addr(actor_record_base: u32) -> u32 {
        actor_record_base + ANIM_DISPATCH_PTR_TABLE_OFFSET
    }

    /// Read the four u32 dispatch pointers from a contiguous main-RAM
    /// slice. Returns `None` if the actor record base is outside the
    /// PSX RAM window or the slice is too short.
    pub fn read_dispatch_pointers(main_ram: &[u8], actor_record_base: u32) -> Option<[u32; 4]> {
        let off =
            (actor_record_base - 0x80000000) as usize + ANIM_DISPATCH_PTR_TABLE_OFFSET as usize;
        let bytes = main_ram.get(off..off + ANIM_DISPATCH_PTR_TABLE_LEN)?;
        let mut out = [0u32; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            let chunk = &bytes[i * 4..i * 4 + 4];
            *slot = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        Some(out)
    }
}

/// Battle item-use observation captured from a save pair: one save
/// with battle just initiated, one save with the active party member
/// using a Healing Leaf (consumable HP-restore item, NOT Fire Book I -
/// the spell-learn writer for the displayed-skills array tracked in
/// PRD §2.6 needs a separate capture).
///
/// The captured pair shares the `map01` battle scene. Both the pre
/// and post saves have the formation cell populated at
/// `0x8007BD0C..0x8007BD0F = 04 04 00 00` (a 2-monster encounter is
/// active across both frames).
///
/// Findings:
///
/// - The post-event save shifts the entire field-pack residency: the
///   loader-base pointer at `_DAT_8007B8D0` flips from `0x8014BD30`
///   (pre-event) to `0x800ABA4C` (post-event), implying the menu /
///   item-use pipeline rebases the active scene asset buffer for
///   the item handler. The bundle pool at `0x80084540` stays on
///   `map01` across both, so this is an internal sub-mode transition
///   rather than a scene swap.
/// - The script-VM context block at `0x801BA7DC..0x801BADEC` shifts
///   wholesale (~660 bytes), consistent with the menu/item dispatch
///   path running to completion (item picker → target picker →
///   action commit → animation queue).
/// - Actor pool slot 0..4 records (5 active actors in the count-2
///   formation: 3 party + 2 monsters) are unchanged in identity but
///   carry per-frame motion state deltas. Slots 5..7 are zeroed in
///   both saves.
/// - The "fire-book +0x185" interpretation gap is NOT closed by this
///   pair - this is a Healing Leaf use, not a spell-learn item, and
///   the displayed-skills writer for `+0x185` does not fire. The
///   pair does pin the **item-use battle-event overlay residency**
///   (the bundle / overlay scratch differences are stable signatures
///   for "an item handler is running") even though the specific
///   Fire Book writer cannot be lifted from these saves alone.
pub mod item_use_battle_event {
    /// Field-pack base pointer cell. Flips between pre / post saves
    /// when the item-use sub-mode reseats the active scene buffer.
    pub const FIELD_PACK_BASE_PTR_ADDR: u32 = 0x8007B8D0;

    /// Pre-event value (battle-init residency).
    pub const FIELD_PACK_BASE_PTR_PRE: u32 = 0x8014BD30;
    /// Post-event value (item-use residency).
    pub const FIELD_PACK_BASE_PTR_POST: u32 = 0x800ABA4C;

    /// Script-VM context block window. ~660 bytes shift across the
    /// pair as the menu / item / target / commit pipeline runs.
    pub const SCRIPT_VM_CTX_WINDOW: (u32, u32) = (0x801BA7DC, 0x801BADEC);

    /// 8-slot battle actor pool. In the count-2 formation, slots 0..4
    /// are populated (3 party + 2 monsters); slots 5..7 are zero in
    /// both saves and remain zero across the pair.
    pub const ACTOR_POOL_BASE: u32 = 0x801C9370;

    /// Number of active actor slots in the count-2 formation.
    pub const ACTIVE_SLOTS: u32 = 5;
    /// Total slots; trailing entries are zero-armed.
    pub const TOTAL_SLOTS: u32 = 8;
}

/// Per-STR FMV trigger corpus: nine save states captured RIGHT before
/// each FMV begins playing, one per `_DAT_8007BA78` value
/// (`fmv_id ∈ 0..=8`).
///
/// Each save is taken at the moment the field-VM (or debug-menu)
/// trigger has just written the next-game-mode global to `0x1A`
/// (`StrInit`) and the FMV index to `_DAT_8007BA78`, but BEFORE the
/// main mode dispatcher swaps in the str_fmv overlay. This means:
///
/// - The trigger-side state (`_DAT_8007BA78` + game mode) is fully
///   pinned and reproducible across the corpus.
/// - The FMV overlay is **NOT** loaded in any of the saves — the
///   compact-table-at-`0x801CAE40` and the runtime FMV-state-table
///   at `0x801D0A6C` aren't visible from these saves alone.
/// - All nine saves were captured from the same active scene
///   (`map01`) via a debug-menu-driven sequence, NOT a per-scene
///   field-VM trigger op. The `0x4C 0xE2 lo hi` byte sequence does
///   NOT appear in the field-pack RAM region for any save (a scan
///   of the 192 KB region following the loader-base pointer turns
///   up zero matches in every save) — this is consistent with the
///   debug menu poking `_DAT_8007BA78` directly rather than with
///   the field VM stepping through a trigger-bearing bytecode
///   buffer.
///
/// **Findings.**
///
/// 1. **`fmv_id` range extends to at least `0..=8`** — six more
///    valid trigger values than the previously-documented
///    `0..=5`. Combined with the static read of the str_fmv
///    overlay's runtime FMV-state table at `0x801D0A6C` from a prior
///    corpus rotation, twelve slots are visible in the table; the
///    first nine are reachable through the debug menu.
/// 2. **Game mode is `0x1A` (StrInit) for every save** — the trigger
///    op writes `_DAT_8007B83C = 0x1A` unconditionally, regardless
///    of which fmv_id was selected.
/// 3. **All saves resolve to `map01`** in the scene-bundle pool
///    (slot 0 = slot 1 = `map01`). The field-pack base
///    (`recover_base`) returns `0x80139530` consistently — a NEW
///    cross-validation point for `map01`'s field-pack RAM
///    residency, as `map01` is one of the seven mid-game
///    FMV-trigger field scenes documented in
///    `legaia_engine_core::scene::FMV_TRIGGER_FIELD_SCENES`.
/// 4. **BGM ID is `2000` (global pool index `0`) for every save** —
///    the FMV trigger path resets the BGM selector to the start of
///    the global pool.
///
/// **Implication for the per-scene MV-index lift (PRD §2.7).** These
/// saves were generated by debug-menu trigger paths, NOT by stepping
/// the field VM through a per-scene FMV trigger op. They therefore
/// pin the `(fmv_id, game_mode)` tuple across the full `0..=8` range
/// but **do not** disambiguate which fmv_id the seven mid-game
/// scenes' field-VM bytecode actually writes at runtime. That
/// per-scene mapping remains gated on a scene-load-time capture of
/// the field-pack preamble's runtime-projected slot.
pub mod cutscene_trigger_corpus {
    /// PSX-virtual address of the FMV-id global written by the
    /// field-VM op `0x4C 0xE2` handler at `0x801E30E4`. The runtime
    /// FMV-state selector at `0x801CECA0` reads this as a `s16`.
    pub const FMV_ID_ADDR: u32 = 0x8007_BA78;

    /// PSX-virtual address of the next-game-mode global. Every
    /// FMV-trigger writer pokes this to `0x1A` (StrInit).
    pub const GAME_MODE_ADDR: u32 = 0x8007_B83C;

    /// Expected game mode value when the corpus saves are loaded.
    /// The main mode dispatcher transitions to mode `26 = StrInit`
    /// on the next frame.
    pub const EXPECTED_GAME_MODE: u8 = 0x1A;

    /// PSX-virtual address of the BGM ID global written by the
    /// field-VM op `0x35` sub-op `1` BGM selector. The trigger path
    /// resets this to `2000` (global pool index `0`) before the
    /// FMV plays.
    pub const BGM_ID_ADDR: u32 = 0x8007_BAC8;

    /// Expected BGM ID value across the corpus. `2000` resolves to
    /// global pool entry `0` per the BGM resolver `FUN_800243F0`.
    pub const EXPECTED_BGM_ID: u16 = 2000;

    /// Expected scene name in the scene-bundle pool (slots 0 + 1).
    /// All nine saves share this label; per-save corpus assertions
    /// can use it as a fast residency check.
    pub const EXPECTED_SCENE_LABEL: &str = "map01";

    /// Expected `recover_base` return value for every save in the
    /// corpus — the `map01` field-pack base. Pins `map01`'s
    /// field-pack runtime residency for cross-referencing against
    /// `FMV_TRIGGER_FIELD_SCENES` and the existing
    /// `field_pack_load::TOWN01_FIELD_PACK_BASE` constant.
    pub const MAP01_FIELD_PACK_BASE: u32 = 0x80139530;

    /// One save in the per-STR FMV corpus.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CorpusEntry {
        /// Mednafen save-state slot suffix (`mc{N}`).
        pub slot: u32,
        /// FMV index the field-VM / debug menu wrote to
        /// [`FMV_ID_ADDR`] before this save was taken.
        pub expected_fmv_id: i16,
    }

    /// Nine corpus entries, one per `fmv_id ∈ 0..=8`. The user-side
    /// slot numbering is `[2,3,4,5,6,7,8,9,0]` mapped to
    /// `expected_fmv_id ∈ 0..=8`.
    pub const CORPUS: [CorpusEntry; 9] = [
        CorpusEntry {
            slot: 2,
            expected_fmv_id: 0,
        },
        CorpusEntry {
            slot: 3,
            expected_fmv_id: 1,
        },
        CorpusEntry {
            slot: 4,
            expected_fmv_id: 2,
        },
        CorpusEntry {
            slot: 5,
            expected_fmv_id: 3,
        },
        CorpusEntry {
            slot: 6,
            expected_fmv_id: 4,
        },
        CorpusEntry {
            slot: 7,
            expected_fmv_id: 5,
        },
        CorpusEntry {
            slot: 8,
            expected_fmv_id: 6,
        },
        CorpusEntry {
            slot: 9,
            expected_fmv_id: 7,
        },
        CorpusEntry {
            slot: 0,
            expected_fmv_id: 8,
        },
    ];

    /// Read the FMV-id global from main RAM (signed 16-bit LE).
    pub fn read_fmv_id(main_ram: &[u8]) -> Option<i16> {
        let off = (FMV_ID_ADDR - 0x80000000) as usize;
        let bytes = main_ram.get(off..off + 2)?;
        Some(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Read the game-mode byte from main RAM.
    pub fn read_game_mode(main_ram: &[u8]) -> Option<u8> {
        let off = (GAME_MODE_ADDR - 0x80000000) as usize;
        main_ram.get(off).copied()
    }

    /// Read the BGM-id global from main RAM (unsigned 16-bit LE).
    pub fn read_bgm_id(main_ram: &[u8]) -> Option<u16> {
        let off = (BGM_ID_ADDR - 0x80000000) as usize;
        let bytes = main_ram.get(off..off + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Search the field-pack region following `field_pack_base` for
    /// the field-VM FMV-trigger op `0x4C 0xE2 lo hi`. Returns each
    /// match as `(absolute_addr, fmv_id_operand)`. Used to confirm
    /// (or refute) that the captured save still has the trigger
    /// bytecode resident — the corpus saves return zero matches, a
    /// stable signature of the debug-menu-driven trigger path.
    pub fn scan_field_pack_for_trigger_ops(
        main_ram: &[u8],
        field_pack_base: u32,
        scan_len: u32,
    ) -> Vec<(u32, i16)> {
        let lo = (field_pack_base - 0x80000000) as usize;
        let hi = (lo + scan_len as usize).min(main_ram.len());
        let bytes = &main_ram[lo..hi];
        let mut out = Vec::new();
        let mut i = 0;
        while i + 3 < bytes.len() {
            if bytes[i] == 0x4C && bytes[i + 1] == 0xE2 {
                let id = i16::from_le_bytes([bytes[i + 2], bytes[i + 3]]);
                out.push((field_pack_base + i as u32, id));
                i += 4;
            } else {
                i += 1;
            }
        }
        out
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

        // Plant the pinned `town01` settled value (`0x8014BD30`) at
        // the right offset.
        let mut ram = vec![0u8; 0x100000];
        let off = (field_pack_load::LOAD_DEST_PLUS_OFFSET_PTR - 0x80000000) as usize;
        ram[off..off + 4].copy_from_slice(&0x8014BD30u32.to_le_bytes());
        let base = field_pack_load::recover_base(&ram).expect("should recover");
        assert_eq!(base, field_pack_load::TOWN01_FIELD_PACK_BASE);
    }

    #[test]
    fn field_pack_constants_round_trip_through_recover() {
        assert_eq!(
            field_pack_load::TOWN01_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET,
            0x8014BD30
        );
        assert_eq!(
            field_pack_load::TOWN0C_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET,
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
        // Plant the mid-transition shape: slot 0 says "town0c",
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

        // Settled state: slot 0 says "town01" + base = PREV_BASE.
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

    #[test]
    fn battle_init_window_extents_consistent() {
        let (lo, hi) = battle_init_overlay::OVERLAY_SCRATCH_WINDOW;
        assert!(hi > lo);
        assert_eq!(hi - lo, 0x4810); // ~16 KB, matches captured diff extent
        let (lo, hi) = battle_init_overlay::BATTLE_BUNDLE_WINDOW;
        assert_eq!(hi - lo, 0x2BD34); // ~168 KB, matches captured diff extent
    }

    #[test]
    fn battle_action_anim_offsets_are_in_actor_record() {
        // Actor record stride = 0x2D4. All anim-state offsets must fall
        // within a single record.
        let stride = 0x2D4u32;
        for &off in &[
            battle_action_animation::ANIM_PC_FIELD_OFFSET,
            battle_action_animation::ANIM_FRAME_FLAGS_OFFSET,
            battle_action_animation::ANIM_DISPATCH_PTR_TABLE_OFFSET,
        ] {
            assert!(
                off < stride,
                "+0x{off:X} should fit in the 0x{stride:X}-byte record"
            );
        }
    }

    #[test]
    fn battle_action_anim_dispatch_table_size_is_4_pointers() {
        assert_eq!(
            battle_action_animation::ANIM_DISPATCH_PTR_TABLE_LEN,
            4 * std::mem::size_of::<u32>()
        );
    }

    #[test]
    fn cutscene_corpus_covers_consecutive_fmv_ids_0_through_8() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in cutscene_trigger_corpus::CORPUS {
            seen.insert(entry.expected_fmv_id);
        }
        let expected: std::collections::BTreeSet<i16> = (0..=8).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn cutscene_corpus_user_slot_assignments_match_capture_intent() {
        // The user captured slot 2 → STR 0, slot 3 → STR 1, ...,
        // slot 0 → STR 8. Encode that fingerprint here so the
        // corpus indices stay synchronised with the on-disc saves.
        let want = [
            (2u32, 0i16),
            (3, 1),
            (4, 2),
            (5, 3),
            (6, 4),
            (7, 5),
            (8, 6),
            (9, 7),
            (0, 8),
        ];
        for (i, entry) in cutscene_trigger_corpus::CORPUS.iter().enumerate() {
            assert_eq!(entry.slot, want[i].0);
            assert_eq!(entry.expected_fmv_id, want[i].1);
        }
    }

    #[test]
    fn cutscene_corpus_readers_lift_planted_values() {
        let mut ram = vec![0u8; 0x200000];
        // Plant fmv_id = 5 (s16 LE).
        let off = (cutscene_trigger_corpus::FMV_ID_ADDR - 0x80000000) as usize;
        ram[off..off + 2].copy_from_slice(&5i16.to_le_bytes());
        assert_eq!(cutscene_trigger_corpus::read_fmv_id(&ram), Some(5));

        // Plant game mode = 0x1A.
        let off = (cutscene_trigger_corpus::GAME_MODE_ADDR - 0x80000000) as usize;
        ram[off] = 0x1A;
        assert_eq!(
            cutscene_trigger_corpus::read_game_mode(&ram),
            Some(cutscene_trigger_corpus::EXPECTED_GAME_MODE)
        );

        // Plant BGM id = 2000.
        let off = (cutscene_trigger_corpus::BGM_ID_ADDR - 0x80000000) as usize;
        ram[off..off + 2].copy_from_slice(&2000u16.to_le_bytes());
        assert_eq!(
            cutscene_trigger_corpus::read_bgm_id(&ram),
            Some(cutscene_trigger_corpus::EXPECTED_BGM_ID)
        );
    }

    #[test]
    fn cutscene_corpus_field_pack_scan_finds_planted_trigger() {
        let mut ram = vec![0u8; 0x200000];
        let base = cutscene_trigger_corpus::MAP01_FIELD_PACK_BASE;
        let off = (base - 0x80000000) as usize;
        // Plant a `0x4C 0xE2 0x05 0x00` trigger op at base + 0x100.
        ram[off + 0x100] = 0x4C;
        ram[off + 0x101] = 0xE2;
        ram[off + 0x102] = 0x05;
        ram[off + 0x103] = 0x00;
        let hits = cutscene_trigger_corpus::scan_field_pack_for_trigger_ops(&ram, base, 0x200);
        assert_eq!(hits, vec![(base + 0x100, 5)]);

        // No matches in a zero-filled RAM image — confirming the
        // corpus's empirical "no trigger op found in field-pack"
        // observation when no op is planted.
        let zero = vec![0u8; 0x200000];
        let no_hits = cutscene_trigger_corpus::scan_field_pack_for_trigger_ops(&zero, base, 0x200);
        assert!(no_hits.is_empty());
    }

    #[test]
    fn cutscene_corpus_map01_field_pack_base_round_trips() {
        // The corpus's pinned map01 field-pack base, plus the
        // EFFECT_OFFSET, should match the load-dest pointer value
        // observed in every save.
        let load_dest =
            cutscene_trigger_corpus::MAP01_FIELD_PACK_BASE + field_pack_load::EFFECT_OFFSET;
        assert_eq!(load_dest, 0x8014BD30);
    }
}
