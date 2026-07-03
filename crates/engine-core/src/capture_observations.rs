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
pub mod encounter_trigger;

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
pub mod vahn_fire_book_use;

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
pub mod field_pack_load;

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
pub mod field_pack_intra_transition;

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
pub mod str_fmv_overlay;

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
/// - `[+0x11C]` HP_max - mirrors live `+0x106`.
/// - `[+0x11E]` MP_max - mirrors live `+0x10A`.
/// - `[+0x120]` per-stat cap constant `100` - unchanged across every
///   captured save and every character.
/// - `[+0x122..+0x12D]` six u16 record-side stats - mirror live
///   `+0x110..+0x11B`.
///
/// SP_max at `+0x10E` lives only in the live in-battle copy. The record
/// at `+0x120` is a 100-cap constant, **not** SP_max. Noa's level-up
/// grants `+40` SP_max (Seru-magic user); Gala's grants `0` (physical
/// Tactical Arts user).
pub mod char_level_up;

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
pub mod battle_init_overlay;

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
pub mod battle_action_animation;

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
pub mod item_use_battle_event;

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
/// - The FMV overlay is **NOT** loaded in any of the saves - the
///   compact-table-at-`0x801CAE40` and the runtime FMV-state-table
///   at `0x801D0A6C` aren't visible from these saves alone.
/// - All nine saves were captured from the same active scene
///   (`map01`) via a debug-menu-driven sequence, NOT a per-scene
///   field-VM trigger op. The `0x4C 0xE2 lo hi` byte sequence does
///   NOT appear in the field-pack RAM region for any save (a scan
///   of the 192 KB region following the loader-base pointer turns
///   up zero matches in every save) - this is consistent with the
///   debug menu poking `_DAT_8007BA78` directly rather than with
///   the field VM stepping through a trigger-bearing bytecode
///   buffer.
///
/// **Findings.**
///
/// 1. **`fmv_id` range extends to at least `0..=8`** - six more
///    valid trigger values than the previously-documented
///    `0..=5`. Combined with the static read of the str_fmv
///    overlay's runtime FMV-state table at `0x801D0A6C` from a prior
///    corpus rotation, twelve slots are visible in the table; the
///    first nine are reachable through the debug menu.
/// 2. **Game mode is `0x1A` (StrInit) for every save** - the trigger
///    op writes `_DAT_8007B83C = 0x1A` unconditionally, regardless
///    of which fmv_id was selected.
/// 3. **All saves resolve to `map01`** in the scene-bundle pool
///    (slot 0 = slot 1 = `map01`). The field-pack base
///    (`recover_base`) returns `0x80139530` consistently - a NEW
///    cross-validation point for `map01`'s field-pack RAM
///    residency, as `map01` is one of the seven mid-game
///    FMV-trigger field scenes documented in
///    `legaia_engine_core::scene::FMV_TRIGGER_FIELD_SCENES`.
/// 4. **BGM ID is `2000` (global pool index `0`) for every save** -
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
pub mod cutscene_trigger_corpus;

/// Seru-capture (spell-learn) observation captured from a before/after
/// save pair during the Rim Elm (`town01`) Gimard fight. `before` is the
/// frame just before the capture lands; `after` is during the "captured!"
/// banner.
///
/// **Finding (pinned).** Across the entire 4-record character table
/// (`0x80084708 .. +4*0x414`) exactly **three** bytes differ between the
/// two saves, all inside Vahn's record (base `0x80084708`):
///
/// ```text
/// +0x13C  00 -> 01    spell-list count: 0 -> 1
/// +0x13D  00 -> 0x81  spell-id array[0] = 0x81 (the spell Gimard teaches)
/// +0x161  00 -> 01    spell-level array[0] = 1
/// ```
///
/// **Interpretation (confirmed).** This validates the
/// [`legaia_save::character::CharacterRecord::spell_list`] schema against
/// retail: the per-character spell list is `[u8 count @ +0x13C][u8 ids[]
/// @ +0x13D..+0x160][u8 levels[] @ +0x161..+0x184]`. Capturing a story
/// Seru (Gimard) is an **immediate** grant - the spell appears at level 1
/// in one step, with **no** capture-points accumulation visible in the
/// record (contrast the optional points-threshold model the engine's
/// [`crate::seru_learning`] approximates for Genocide-Crystal captures).
///
/// **Real spell ids run high.** Gimard's spell id `0x81` sits far above the
/// clean-room [`crate::spells::SpellCatalog::vanilla`] id range
/// (`0x10..=0x51`). The retail spell-id space is therefore distinct from
/// the engine's placeholder catalog - a single data point, not yet a full
/// re-map.
///
/// Catalogued saves: `rim_elm_gimard_seru_capture_before` /
/// `rim_elm_gimard_seru_capture_after` in `scripts/scenarios.toml`
/// (library-only, resolved by `backup_fingerprint`).
pub mod seru_capture;

#[cfg(test)]
mod tests;
